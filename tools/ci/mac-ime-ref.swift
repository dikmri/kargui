// 日本語入力を比べる相手: NSTextInputClient を持つ最小の NSView（SDL・GLFW と同じ答え方）を、AppKit の標準の輪
// （[NSApp run]）で動かし、IME の呼び返しを標準出力に書く（.github/workflows/ci.yml の macos-window の段が使う）。
// 25 秒で終わる。窓の中身は 400 × 200。

import AppKit

func say(_ s: String) {
    print("\(Date().timeIntervalSince1970) \(s)")
    fflush(stdout)
}

final class RefView: NSView, NSTextInputClient {
    var marked = NSMutableAttributedString()

    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        let handled = inputContext?.handleEvent(event) ?? false
        say("keyDown code=\(event.keyCode) handled=\(handled) source=\(inputContext?.selectedKeyboardInputSource ?? "none")")
        if !handled {
            interpretKeyEvents([event])
        }
    }

    func text(_ s: Any) -> String {
        (s as? NSAttributedString)?.string ?? (s as? String) ?? "?"
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        say("insertText \(text(string))")
        marked = NSMutableAttributedString()
    }

    override func doCommand(by selector: Selector) {
        say("doCommand \(selector)")
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        say("setMarkedText \(text(string)) selected=\(selectedRange.location)+\(selectedRange.length)")
        marked = NSMutableAttributedString(string: text(string))
    }

    func unmarkText() {
        say("unmarkText")
        marked = NSMutableAttributedString()
    }

    func selectedRange() -> NSRange { NSRange(location: NSNotFound, length: 0) }

    func markedRange() -> NSRange {
        marked.length > 0 ? NSRange(location: 0, length: marked.length) : NSRange(location: NSNotFound, length: 0)
    }

    func hasMarkedText() -> Bool { marked.length > 0 }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? { nil }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let w = window else { return .zero }
        return w.convertToScreen(convert(NSRect(x: 20, y: 20, width: 1, height: 20), to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int { 0 }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 400, height: 200), styleMask: [.titled, .closable], backing: .buffered, defer: false)
let view = RefView(frame: NSRect(x: 0, y: 0, width: 400, height: 200))
win.contentView = view
win.title = "ime-ref"
win.center()
win.makeKeyAndOrderFront(nil)
win.makeFirstResponder(view)
app.activate(ignoringOtherApps: true)
say("started bundle=\(Bundle.main.bundleIdentifier ?? "none")")
DispatchQueue.main.asyncAfter(deadline: .now() + 25) {
    say("quit")
    app.terminate(nil)
}
app.run()
