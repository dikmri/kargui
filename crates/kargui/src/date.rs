//! 日付（年・月・日）と時刻（時・分）、その計算。依存を増やさないため、暦の計算は自分で持つ。
//! 年月日と 1970-01-01 からの日数の変換は、Howard Hinnant の days_from_civil / civil_from_days
//! （グレゴリオ暦を過去にも延ばした暦）と同じ考え方で書いた。

use std::fmt;

/// A time of day, hours (0 to 23) and minutes (0 to 59), shown as `09:30`.
///
/// ```
/// use kargui::Time;
///
/// let t = Time::new(9, 30).unwrap();
/// assert_eq!(t.to_string(), "09:30");
/// assert_eq!(t.add_minutes(45), Time::new(10, 15).unwrap());
/// assert_eq!(Time::new(23, 50).unwrap().add_minutes(20), Time::new(0, 10).unwrap()); // past midnight
/// assert_eq!(Time::parse("7:05"), Time::new(7, 5));
/// assert!(Time::new(24, 0).is_none());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Time {
    hour: u8,
    minute: u8,
}

impl Time {
    /// The time, or `None` if the hour is not 0 to 23 or the minute not 0 to 59.
    ///
    /// ```
    /// assert!(kargui::Time::new(23, 59).is_some());
    /// assert!(kargui::Time::new(12, 60).is_none());
    /// ```
    pub fn new(hour: u32, minute: u32) -> Option<Time> {
        (hour < 24 && minute < 60).then_some(Time { hour: hour as u8, minute: minute as u8 })
    }

    /// The time now in the local time zone (on Windows; on other systems, in UTC for now).
    ///
    /// ```
    /// let now = kargui::Time::now();
    /// assert!(now.hour() < 24);
    /// ```
    pub fn now() -> Time {
        #[cfg(windows)]
        if let Some(t) = crate::platform::local_time().and_then(|(h, m)| Time::new(h, m)) {
            return t;
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        Time::from_minutes(secs.div_euclid(60))
    }

    /// Reads `9:30` or `09:30` (24-hour). `None` if the text is not a time.
    ///
    /// ```
    /// use kargui::Time;
    ///
    /// assert_eq!(Time::parse("18:45"), Time::new(18, 45));
    /// assert_eq!(Time::parse(" 9:05 "), Time::new(9, 5));
    /// assert_eq!(Time::parse("9:5"), None);
    /// assert_eq!(Time::parse("25:00"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Time> {
        let (h, m) = text.trim().split_once(':')?;
        let digits = |s: &str, max_len: usize| (!s.is_empty() && s.len() <= max_len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<u32>().ok()).flatten();
        if m.len() != 2 {
            return None;
        }
        Time::new(digits(h, 2)?, digits(m, 2)?)
    }

    /// The hour, 0 to 23.
    ///
    /// ```
    /// assert_eq!(kargui::Time::new(18, 45).unwrap().hour(), 18);
    /// ```
    pub fn hour(self) -> u32 {
        self.hour as u32
    }

    /// The minute, 0 to 59.
    ///
    /// ```
    /// assert_eq!(kargui::Time::new(18, 45).unwrap().minute(), 45);
    /// ```
    pub fn minute(self) -> u32 {
        self.minute as u32
    }

    /// Minutes since midnight, 0 to 1439.
    ///
    /// ```
    /// assert_eq!(kargui::Time::new(1, 30).unwrap().minutes(), 90);
    /// ```
    pub fn minutes(self) -> u32 {
        self.hour as u32 * 60 + self.minute as u32
    }

    /// The time `minutes` after midnight, going round the clock (1440 is midnight again, -10 is
    /// 23:50).
    ///
    /// ```
    /// use kargui::Time;
    ///
    /// assert_eq!(Time::from_minutes(90), Time::new(1, 30).unwrap());
    /// assert_eq!(Time::from_minutes(-10), Time::new(23, 50).unwrap());
    /// ```
    pub fn from_minutes(minutes: i64) -> Time {
        let m = minutes.rem_euclid(24 * 60) as u32;
        Time { hour: (m / 60) as u8, minute: (m % 60) as u8 }
    }

    /// The time `minutes` later (earlier if negative), going round the clock.
    ///
    /// ```
    /// use kargui::Time;
    ///
    /// assert_eq!(Time::new(0, 5).unwrap().add_minutes(-10), Time::new(23, 55).unwrap());
    /// ```
    pub fn add_minutes(self, minutes: i64) -> Time {
        Time::from_minutes(self.minutes() as i64 + minutes)
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// A calendar date (the Gregorian calendar), shown as `2026-11-22`.
///
/// ```
/// use kargui::Date;
///
/// let d = Date::new(2026, 11, 22).unwrap();
/// assert_eq!(d.to_string(), "2026-11-22");
/// assert_eq!(d.weekday(), 0); // Sunday
/// assert_eq!(d.add_days(10), Date::new(2026, 12, 2).unwrap());
/// assert!(Date::new(2026, 2, 29).is_none());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i32,
    month: u32,
    day: u32,
}

impl Date {
    /// The date, or `None` if there is no such day (such as February 30).
    ///
    /// ```
    /// assert!(kargui::Date::new(2028, 2, 29).is_some()); // a leap year
    /// assert!(kargui::Date::new(2026, 13, 1).is_none());
    /// ```
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month)).then_some(Date { year, month, day })
    }

    /// Today's date in the local time zone (on Windows; on other systems, in UTC for now).
    ///
    /// ```
    /// let today = kargui::Date::today();
    /// assert!(today.year() >= 2024);
    /// ```
    pub fn today() -> Date {
        // 地元の日付は OS に聞く（OS を呼ぶのは境目のファイルだけ）
        #[cfg(windows)]
        if let Some(d) = crate::platform::local_date().and_then(|(y, m, d)| Date::new(y, m, d)) {
            return d;
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        Date::from_days(secs.div_euclid(86_400))
    }

    /// The year.
    ///
    /// ```
    /// assert_eq!(kargui::Date::new(2026, 11, 22).unwrap().year(), 2026);
    /// ```
    pub fn year(self) -> i32 {
        self.year
    }

    /// The month, 1 (January) to 12 (December).
    ///
    /// ```
    /// assert_eq!(kargui::Date::new(2026, 11, 22).unwrap().month(), 11);
    /// ```
    pub fn month(self) -> u32 {
        self.month
    }

    /// The day of the month, from 1.
    ///
    /// ```
    /// assert_eq!(kargui::Date::new(2026, 11, 22).unwrap().day(), 22);
    /// ```
    pub fn day(self) -> u32 {
        self.day
    }

    /// The day of the week: 0 is Sunday, 1 Monday, ... 6 Saturday.
    ///
    /// ```
    /// assert_eq!(kargui::Date::new(2026, 11, 23).unwrap().weekday(), 1); // Monday
    /// ```
    pub fn weekday(self) -> u32 {
        // 1970-01-01 は木曜日（4）
        (self.days() + 4).rem_euclid(7) as u32
    }

    /// The date `n` days later (earlier if `n` is negative).
    ///
    /// ```
    /// use kargui::Date;
    /// assert_eq!(Date::new(2026, 12, 31).unwrap().add_days(1), Date::new(2027, 1, 1).unwrap());
    /// ```
    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.days() + n)
    }

    /// The same day `n` months later (earlier if negative); a day that does not exist in that
    /// month becomes its last day.
    ///
    /// ```
    /// use kargui::Date;
    /// assert_eq!(Date::new(2026, 1, 31).unwrap().add_months(1), Date::new(2026, 2, 28).unwrap());
    /// ```
    pub fn add_months(self, n: i32) -> Date {
        let m = self.year as i64 * 12 + (self.month as i64 - 1) + n as i64;
        let (year, month) = (m.div_euclid(12) as i32, m.rem_euclid(12) as u32 + 1);
        Date { year, month, day: self.day.min(days_in_month(year, month)) }
    }

    /// The number of days in this date's month.
    ///
    /// ```
    /// assert_eq!(kargui::Date::new(2028, 2, 1).unwrap().days_in_month(), 29);
    /// ```
    pub fn days_in_month(self) -> u32 {
        days_in_month(self.year, self.month)
    }

    /// The date `days` days after 1970-01-01 (before it if negative). With the seconds of a
    /// [`SystemTime`](std::time::SystemTime) since the Unix epoch divided by 86,400, this is that
    /// day in UTC.
    ///
    /// ```
    /// use kargui::Date;
    /// assert_eq!(Date::from_unix_days(0), Date::new(1970, 1, 1).unwrap());
    /// assert_eq!(Date::from_unix_days(20_414), Date::new(2025, 11, 22).unwrap());
    /// ```
    pub fn from_unix_days(days: i64) -> Date {
        Date::from_days(days)
    }

    /// The number of days since 1970-01-01. See [`Date::from_unix_days`].
    ///
    /// ```
    /// use kargui::Date;
    /// let d = Date::new(2026, 11, 22).unwrap();
    /// assert_eq!(Date::from_unix_days(d.unix_days()), d);
    /// ```
    pub fn unix_days(self) -> i64 {
        self.days()
    }

    /// 1970-01-01 からの日数
    pub(crate) fn days(self) -> i64 {
        let (m, d) = (self.month as i64, self.day as i64);
        let y = self.year as i64 - if m <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// 1970-01-01 からの日数から日付へ
    pub(crate) fn from_days(z: i64) -> Date {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
        Date { year, month, day }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// その月の日数
fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// The words date and time pickers show: names of the days and months, the order of the month
/// title, the text of an empty field, and the names of the hour and minute grids. Set them for
/// the whole window with
/// [`Ui::set_date_text`](crate::Ui::set_date_text).
///
/// ```
/// use kargui::DateText;
/// let ja = DateText::japanese();
/// assert_eq!(ja.weekdays[0], "日");
/// let mut mine = DateText::english();
/// mine.empty = "No due date";
/// # let _ = mine;
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct DateText {
    /// Short names of the days of the week, Sunday first.
    pub weekdays: [&'static str; 7],
    /// Names of the months, January first.
    pub months: [&'static str; 12],
    /// Writes the year before the month (`2026年11月`) instead of after (`November 2026`).
    pub year_first: bool,
    /// The text of a field with no date.
    pub empty: &'static str,
    /// The names of the buttons that show the previous and the next month.
    pub previous: &'static str,
    /// See `previous`.
    pub next: &'static str,
    /// The text of a time field with no time ([`Ui::time_picker`](crate::Ui::time_picker)).
    pub time_empty: &'static str,
    /// The names of the grids of hours and minutes in a time picker.
    pub hours: &'static str,
    /// See `hours`.
    pub minutes: &'static str,
}

impl DateText {
    /// English (the default).
    ///
    /// ```
    /// assert_eq!(kargui::DateText::english().months[10], "November");
    /// ```
    pub fn english() -> DateText {
        DateText {
            weekdays: ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"],
            months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
            year_first: false,
            empty: "Pick a date",
            previous: "Previous month",
            next: "Next month",
            time_empty: "Pick a time",
            hours: "Hours",
            minutes: "Minutes",
        }
    }

    /// Japanese.
    ///
    /// ```
    /// assert_eq!(kargui::DateText::japanese().empty, "日付を選ぶ");
    /// ```
    pub fn japanese() -> DateText {
        DateText {
            weekdays: ["日", "月", "火", "水", "木", "金", "土"],
            months: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
            year_first: true,
            empty: "日付を選ぶ",
            previous: "前の月",
            next: "次の月",
            time_empty: "時刻を選ぶ",
            hours: "時",
            minutes: "分",
        }
    }

    /// 月の題（November 2026 / 2026年11月）
    pub(crate) fn title(&self, year: i32, month: u32) -> String {
        let m = self.months[(month - 1) as usize];
        if self.year_first { format!("{year}年{m}") } else { format!("{m} {year}") }
    }

    /// 日の名前（読み上げ用。November 22, 2026 / 2026年11月22日）
    pub(crate) fn full(&self, d: Date) -> String {
        let m = self.months[(d.month - 1) as usize];
        if self.year_first { format!("{}年{m}{}日", d.year, d.day) } else { format!("{m} {}, {}", d.day, d.year) }
    }
}

impl Default for DateText {
    fn default() -> Self {
        DateText::english()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_dates_convert_both_ways() {
        assert_eq!(Date::new(1970, 1, 1).unwrap().days(), 0);
        assert_eq!(Date::new(2000, 3, 1).unwrap().days(), 11_017);
        for z in [-800_000, -1, 0, 1, 10_957, 20_000, 2_932_896] {
            assert_eq!(Date::from_days(z).days(), z, "{z}");
        }
        assert_eq!(Date::from_days(-1), Date::new(1969, 12, 31).unwrap());
    }

    #[test]
    fn leap_years_month_lengths_and_weekdays() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2100, 2, 29).is_none(), "100 で割り切れる年は閏年でない");
        assert!(Date::new(2000, 2, 29).is_some(), "400 で割り切れる年は閏年");
        assert!(Date::new(2026, 4, 31).is_none());
        assert_eq!(Date::new(2026, 11, 22).unwrap().weekday(), 0);
        assert_eq!(Date::new(2026, 1, 1).unwrap().weekday(), 4);
        assert_eq!(Date::new(2026, 3, 31).unwrap().add_months(-1), Date::new(2026, 2, 28).unwrap());
        assert_eq!(Date::new(2026, 1, 15).unwrap().add_months(-13), Date::new(2024, 12, 15).unwrap());
        assert_eq!(Date::new(2027, 1, 1).unwrap().add_days(-1), Date::new(2026, 12, 31).unwrap());
    }

    #[test]
    fn times_wrap_round_the_clock_and_parse_24_hour_text() {
        assert_eq!(Time::new(0, 0).unwrap().minutes(), 0);
        assert_eq!(Time::new(23, 59).unwrap().minutes(), 1439);
        assert_eq!(Time::from_minutes(1440), Time::new(0, 0).unwrap());
        assert_eq!(Time::from_minutes(-1), Time::new(23, 59).unwrap());
        assert_eq!(Time::new(9, 30).unwrap().add_minutes(-600), Time::new(23, 30).unwrap());
        assert_eq!(Time::new(7, 5).unwrap().to_string(), "07:05");
        for bad in ["", "930", "9:", ":30", "9:3", "9:300", "a9:30", "09:3x", "24:00", "12:60", "123:00"] {
            assert_eq!(Time::parse(bad), None, "{bad:?}");
        }
        assert_eq!(Time::parse("00:00"), Time::new(0, 0));
        assert!(Time::new(12, 0).unwrap() < Time::new(12, 1).unwrap(), "時刻は早い順に並ぶ");
    }

    #[test]
    fn date_text_names_titles_and_days() {
        let en = DateText::english();
        let ja = DateText::japanese();
        let d = Date::new(2026, 11, 22).unwrap();
        assert_eq!(en.title(2026, 11), "November 2026");
        assert_eq!(ja.title(2026, 11), "2026年11月");
        assert_eq!(en.full(d), "November 22, 2026");
        assert_eq!(ja.full(d), "2026年11月22日");
    }
}
