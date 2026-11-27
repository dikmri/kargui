// CI の Mac で、kargui の窓へ本物のマウスとキーの入力（CGEvent）を送る。作者は Mac を持たないので、
// macOS の入力の確かめはここで行う（.github/workflows/ci.yml の macos-window の段が使う）。
//
// 使い方: mac-drive <pid> <窓の中の高さ（論理ピクセル）> 手順...
//   move:X,Y      ポインターを動かす
//   click:X,Y     左のボタンで 1 回押す
//   dclick:X,Y    左のボタンで 2 回続けて押す
//   rclick:X,Y    右のボタンで 1 回押す
//   down:X,Y      左のボタンを押したままにする
//   dragto:X,Y    押したまま、今の場所からそこまで 10 回に分けて動かす
//   up:X,Y        左のボタンを放す
//   wheel:X,Y,N   ホイールを N 行回す（正は上へ、負は下へ送る）
//   key:名前      キーを 1 回押す（cmd+a・shift+tab・tab・enter・backspace・left など）
//   type:文字     英小文字・数字・空白は本物のキーで、ほかは Unicode の文字として送る
//   screen:場所   画面を撮る（screencapture）
//   bounds        窓の外枠を書き出し、窓の中の左上を読み直す（窓を動かした後の手順の座標が合う）
//   inputsource:ID  入力ソースを入れて切り替える（例: com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese は日本語の
//                 ローマ字入力、com.apple.keylayout.ABC は英字）。入力モードの時は、親の入力ソースも入れる
//   wait:ミリ秒   待つ
// X,Y は窓の中の左上からの論理ピクセル（題の帯は負の Y）。窓の場所は、最初に CGWindowList で pid の窓を探して決める。

import AppKit
import Carbon
import CoreGraphics
import Foundation

let iso = ISO8601DateFormatter()
iso.formatOptions = [.withInternetDateTime, .withFractionalSeconds]

func say(_ s: String) {
    print("\(iso.string(from: Date())) \(s)")
    fflush(stdout)
}

func frontmost() -> String {
    NSWorkspace.shared.frontmostApplication?.localizedName ?? "none"
}

let args = CommandLine.arguments
guard args.count >= 3, let pid = Int(args[1]), let contentHeight = Double(args[2]) else {
    say("usage: mac-drive <pid> <content-height> steps...")
    exit(2)
}

say("access post_event=\(CGPreflightPostEventAccess()) screen_capture=\(CGPreflightScreenCaptureAccess())")

/// pid の窓の外枠（画面の座標。左上が原点、論理ピクセル。題の帯を含む）
func windowRect() -> CGRect? {
    guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] else { return nil }
    for w in list {
        guard let owner = w[kCGWindowOwnerPID as String] as? Int, owner == pid else { continue }
        guard let layer = w[kCGWindowLayer as String] as? Int, layer == 0 else { continue }
        guard let b = w[kCGWindowBounds as String] as? NSDictionary, let r = CGRect(dictionaryRepresentation: b as CFDictionary) else { continue }
        return r
    }
    return nil
}

func describe(_ r: CGRect) -> String {
    "x=\(Int(r.minX)) y=\(Int(r.minY)) w=\(Int(r.width)) h=\(Int(r.height))"
}

var found: CGRect? = nil
for _ in 0..<100 {
    if let r = windowRect() {
        found = r
        break
    }
    usleep(100_000)
}
guard let first = found else {
    say("window not found pid=\(pid)")
    exit(1)
}
/// 題の帯の高さ（外枠の高さ − 中の高さ）
let titleBar = first.height - contentHeight
/// 窓の中の左上（画面の座標）。bounds の手順で読み直す
var origin = CGPoint(x: first.minX, y: first.minY + titleBar)
say("window \(describe(first)) title_bar=\(Int(titleBar)) content_origin=\(Int(origin.x)),\(Int(origin.y)) frontmost=\(frontmost())")

let source = CGEventSource(stateID: .hidSystemState)

func post(_ e: CGEvent?) {
    guard let e = e else {
        say("  event=nil")
        return
    }
    e.post(tap: .cghidEventTap)
}

func mouse(_ type: CGEventType, _ p: CGPoint, _ button: CGMouseButton = .left, clicks: Int64 = 0) {
    let e = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: p, mouseButton: button)
    if clicks > 0 {
        e?.setIntegerValueField(.mouseEventClickState, value: clicks)
    }
    post(e)
}

/// US の配列のキーの場所の番号（kVK_）
let keyCodes: [String: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14,
    "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23, "9": 25, "7": 26, "8": 28, "0": 29,
    "o": 31, "u": 32, "i": 34, "p": 35, "l": 37, "j": 38, "k": 40, "n": 45, "m": 46, " ": 49,
    "enter": 36, "tab": 48, "space": 49, "backspace": 51, "escape": 53, "delete": 117,
    "left": 123, "right": 124, "down": 125, "up": 126, "home": 115, "end": 119,
]

/// Unicode の文字を送る時のキー（ISO の § の場所。kargui はキーとしては受け取らず、文字だけを受け取る）
let unicodeKey: CGKeyCode = 10

func key(_ code: CGKeyCode, _ flags: CGEventFlags = [], unicode: [UniChar]? = nil) {
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: down)
        e?.flags = flags
        if let u = unicode {
            e?.keyboardSetUnicodeString(stringLength: u.count, unicodeString: u)
        }
        post(e)
        usleep(30_000)
    }
}

/// 入力ソースの文字の属性（ID など）
func sourceProperty(_ s: TISInputSource, _ key: CFString) -> String? {
    guard let p = TISGetInputSourceProperty(s, key) else { return nil }
    return Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String
}

func currentSource() -> String {
    guard let s = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue() else { return "none" }
    return sourceProperty(s, kTISPropertyInputSourceID) ?? "?"
}

/// 入っている入力ソースすべて（入れていない物も含む）
func allSources() -> [TISInputSource] {
    guard let list = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] else { return [] }
    return list
}

/// ID の入力ソースを入れて切り替える。入力モード（親の ID + "." + モード）なら親も入れる
func selectSource(_ id: String) -> String {
    let sources = allSources()
    let parent = id.split(separator: ".").dropLast().joined(separator: ".")
    var log: [String] = []
    if let p = sources.first(where: { sourceProperty($0, kTISPropertyInputSourceID) == parent }) {
        log.append("enable_parent=\(TISEnableInputSource(p))")
    }
    guard let src = sources.first(where: { sourceProperty($0, kTISPropertyInputSourceID) == id }) else {
        let near = sources.compactMap { sourceProperty($0, kTISPropertyInputSourceID) }.filter { $0.contains("Kotoeri") || $0.contains("Japanese") || $0.contains("ABC") }
        return "not found; similar=\(near)"
    }
    log.append("enable=\(TISEnableInputSource(src))")
    log.append("select=\(TISSelectInputSource(src))")
    usleep(300_000)
    log.append("current=\(currentSource())")
    return log.joined(separator: " ")
}

/// 押したままの間の、今のポインターの場所
var pointer = origin

for step in args.dropFirst(3) {
    let parts = step.split(separator: ":", maxSplits: 1)
    let kind = parts.first.map(String.init) ?? ""
    let rest = parts.count > 1 ? String(parts[1]) : ""
    let nums = rest.split(separator: ",").compactMap { Double($0) }
    let p = nums.count >= 2 ? CGPoint(x: origin.x + nums[0], y: origin.y + nums[1]) : origin
    switch kind {
    case "move":
        mouse(.mouseMoved, p)
    case "click":
        mouse(.mouseMoved, p)
        usleep(80_000)
        mouse(.leftMouseDown, p, clicks: 1)
        usleep(60_000)
        mouse(.leftMouseUp, p, clicks: 1)
    case "dclick":
        mouse(.mouseMoved, p)
        usleep(80_000)
        for n in [Int64(1), 2] {
            mouse(.leftMouseDown, p, clicks: n)
            usleep(40_000)
            mouse(.leftMouseUp, p, clicks: n)
            usleep(60_000)
        }
    case "down":
        mouse(.mouseMoved, p)
        usleep(80_000)
        mouse(.leftMouseDown, p, clicks: 1)
        pointer = p
    case "dragto":
        for i in 1...10 {
            let t = Double(i) / 10
            let q = CGPoint(x: pointer.x + (p.x - pointer.x) * t, y: pointer.y + (p.y - pointer.y) * t)
            mouse(.leftMouseDragged, q)
            usleep(30_000)
        }
        pointer = p
    case "up":
        mouse(.leftMouseUp, p, clicks: 1)
    case "rclick":
        mouse(.mouseMoved, p)
        usleep(80_000)
        mouse(.rightMouseDown, p, .right, clicks: 1)
        usleep(60_000)
        mouse(.rightMouseUp, p, .right, clicks: 1)
    case "wheel":
        mouse(.mouseMoved, p)
        usleep(80_000)
        let lines = nums.count >= 3 ? Int32(nums[2]) : -1
        let e = CGEvent(scrollWheelEvent2Source: source, units: .line, wheelCount: 1, wheel1: lines, wheel2: 0, wheel3: 0)
        e?.location = p
        post(e)
    case "key":
        var flags: CGEventFlags = []
        var name = ""
        for piece in rest.split(separator: "+") {
            switch piece {
            case "cmd": flags.insert(.maskCommand)
            case "shift": flags.insert(.maskShift)
            case "ctrl": flags.insert(.maskControl)
            case "alt", "option": flags.insert(.maskAlternate)
            default: name = String(piece)
            }
        }
        guard let code = keyCodes[name] else {
            say("step \(step) unknown key")
            continue
        }
        key(code, flags)
    case "type":
        for c in rest {
            if let code = keyCodes[String(c)] {
                key(code)
            } else {
                key(unicodeKey, unicode: Array(String(c).utf16))
            }
        }
    case "inputsource":
        say("step \(step) \(selectSource(rest))")
        continue
    case "bounds":
        guard let r = windowRect() else {
            say("step \(step) window not found")
            continue
        }
        origin = CGPoint(x: r.minX, y: r.minY + titleBar)
        say("step \(step) window \(describe(r)) content_origin=\(Int(origin.x)),\(Int(origin.y))")
        continue
    case "screen":
        let shot = Process()
        shot.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        shot.arguments = ["-x", rest]
        do {
            try shot.run()
            shot.waitUntilExit()
        } catch {
            say("step \(step) failed: \(error)")
            continue
        }
    case "wait":
        usleep(UInt32((nums.first ?? 0) * 1000))
    default:
        say("step \(step) unknown")
        continue
    }
    say("step \(step) sent frontmost=\(frontmost())")
    usleep(150_000)
}
say("done frontmost=\(frontmost())")
