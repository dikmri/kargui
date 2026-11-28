//! macOS の通知（UNUserNotificationCenter）。作者の決定（2026-11-28「束のアプリだけで動く形で作る」）。
//!
//! 通知は .app の束（Info.plist に CFBundleIdentifier がある）のアプリだけで使える。束でない時（cargo run など）に
//! 通知の中心を作ると Objective-C の例外で止まるので、作らずに記録 notify.unavailable を書いて何もしない。
//! 初めての通知で許可を頼み（OS が許可を聞く）、ボタンは分類（UNNotificationCategory）として登録してから出す。
//! 同じキーの通知は前の通知を置き換える（Windows・Linux と同じ）。押された答えは委任先が受け取る。委任先が呼ばれる
//! スレッドは決まっていないので、答えは Mutex の列に積み、輪を起こす（窓の処理が頼んだ窓に入れる）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSArray, NSBundle, NSError, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationDefaultActionIdentifier, UNNotificationDismissActionIdentifier, UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse, UNNotificationSound,
    UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use crate::log;
use crate::ui::notify::{Notification, NotificationAction};

/// 押された答え（通知のキー, 答え）。委任先が積み、輪が取り出す
type Answers = Arc<Mutex<Vec<(String, NotificationAction)>>>;

struct DelegateIvars {
    answers: Answers,
    wake: fn(),
}

define_class!(
    // SAFETY: NSObject を派生させるだけで、dealloc を書き換えない
    #[unsafe(super(NSObject))]
    #[name = "KarguiNotifyDelegate"]
    #[ivars = DelegateIvars]
    struct NotifyDelegate;

    unsafe impl NSObjectProtocol for NotifyDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotifyDelegate {
        /// アプリが前面の間も通知を出す（何もしないと、前面のアプリの通知は出さない）
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(&self, _center: &UNUserNotificationCenter, _notification: &UNNotification, handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
            handler.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List | UNNotificationPresentationOptions::Sound,));
        }

        /// 通知の本体かボタンが押された（閉じただけの時は答えにしない）
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, handler: &block2::DynBlock<dyn Fn()>) {
            let key = response.notification().request().identifier().to_string();
            let action = response.actionIdentifier();
            // SAFETY: UserNotifications が持つ定数の NSString を読むだけ
            let (body, dismiss) = unsafe { (UNNotificationDefaultActionIdentifier, UNNotificationDismissActionIdentifier) };
            let answer = if action.isEqualToString(body) {
                Some(NotificationAction::Body)
            } else if action.isEqualToString(dismiss) {
                None
            } else {
                Some(NotificationAction::Button(action.to_string()))
            };
            log::event("notify.response", &format!("key={key} action={action}"), &format!("{answer:?}"));
            if let Some(a) = answer {
                if let Ok(mut list) = self.ivars().answers.lock() {
                    list.push((key, a));
                }
                (self.ivars().wake)();
            }
            handler.call(());
        }
    }
);

/// 通知の係（最初の通知で作る）
struct MacNotifier {
    center: Retained<UNUserNotificationCenter>,
    /// 委任先（通知の中心は弱く持つので、ここで持ち続ける）
    _delegate: Retained<NotifyDelegate>,
    answers: Answers,
    /// 登録したボタンの分類（分類の id → 分類。登録は全部をまとめて置き換える形なので、覚えておく）
    categories: HashMap<String, Retained<UNNotificationCategory>>,
    /// 通知のキー → 頼んだ窓（None は主の窓）
    owners: HashMap<String, Option<String>>,
}

thread_local! {
    /// 通知の係。外の None はまだ作っていない、内の None は束でないので使えない
    static NOTIFIER: RefCell<Option<Option<MacNotifier>>> = const { RefCell::new(None) };
}

impl MacNotifier {
    /// 束のアプリなら作り、許可を頼む。束でなければ None
    fn new(wake: fn()) -> Option<Self> {
        let Some(bundle) = NSBundle::mainBundle().bundleIdentifier() else {
            log::event("notify.unavailable", "no CFBundleIdentifier", "notifications need an .app bundle on macOS; nothing is shown");
            return None;
        };
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let answers: Answers = Arc::new(Mutex::new(Vec::new()));
        let delegate = NotifyDelegate::alloc().set_ivars(DelegateIvars { answers: answers.clone(), wake });
        // SAFETY: NSObject の init を呼ぶだけ（ivars は上で入れた）
        let delegate: Retained<NotifyDelegate> = unsafe { msg_send![super(delegate), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let asked = RcBlock::new(|granted: Bool, error: *mut NSError| {
            log::event("notify.authorization", "", &format!("granted={} error={}", granted.as_bool(), describe(error)));
        });
        center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &asked);
        log::event("notify.start", &format!("bundle={bundle}"), "authorization requested");
        Some(Self { center, _delegate: delegate, answers, categories: HashMap::new(), owners: HashMap::new() })
    }

    fn show(&mut self, owner: Option<String>, key: String, n: &Notification) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&n.title));
        if !n.body.is_empty() {
            content.setBody(&NSString::from_str(&n.body));
        }
        if !n.silent {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        if !n.buttons.is_empty() {
            let id = format!("kargui.{key}");
            let actions: Vec<Retained<UNNotificationAction>> = n
                .buttons
                .iter()
                .map(|(bid, label)| UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str(bid), &NSString::from_str(label), UNNotificationActionOptions::empty()))
                .collect();
            let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(&id),
                &NSArray::from_retained_slice(&actions),
                &NSArray::<NSString>::new(),
                UNNotificationCategoryOptions::empty(),
            );
            self.categories.insert(id.clone(), category);
            let all: Vec<Retained<UNNotificationCategory>> = self.categories.values().cloned().collect();
            self.center.setNotificationCategories(&NSSet::from_retained_slice(&all));
            content.setCategoryIdentifier(&NSString::from_str(&id));
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&key), &content, None);
        let k = key.clone();
        let added = RcBlock::new(move |error: *mut NSError| {
            log::event("notify.added", &format!("key={k}"), &describe(error));
        });
        self.center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
        self.owners.insert(key, owner);
    }
}

/// 誤りの説明（無ければ ok）
fn describe(error: *mut NSError) -> String {
    if error.is_null() {
        return "ok".to_string();
    }
    // SAFETY: OS が完了の block に渡した NSError（呼ばれている間は生きている）
    let e = unsafe { &*error };
    format!("error domain={} code={} {}", e.domain(), e.code(), e.localizedDescription())
}

/// 通知を出す（最初の通知で係を作る。束でなければ何もしない）。owner は頼んだ窓（None は主の窓）
pub(crate) fn show(owner: Option<String>, key: String, n: &Notification, wake: fn()) {
    NOTIFIER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let notifier = slot.get_or_insert_with(|| MacNotifier::new(wake));
        match notifier {
            Some(m) => m.show(owner, key, n),
            None => log::event("notify.skipped", &format!("key={key}"), "no .app bundle"),
        }
    });
}

/// 押された答えを取り出す（頼んだ窓, 通知のキー, 答え）
pub(crate) fn take_answers() -> Vec<(Option<String>, String, NotificationAction)> {
    NOTIFIER.with(|cell| {
        let slot = cell.borrow();
        let Some(Some(m)) = slot.as_ref() else { return Vec::new() };
        let list: Vec<(String, NotificationAction)> = m.answers.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default();
        list.into_iter().map(|(key, a)| (m.owners.get(&key).cloned().flatten(), key, a)).collect()
    })
}
