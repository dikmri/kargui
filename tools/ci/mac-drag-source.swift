// ほかのアプリからファイルを引っ張って落とすのを、CI の Mac で真似る（.github/workflows/ci.yml の macos-window の段が使う）。
// 縁の無い小さな窓（画面の左上から 40,80、120 × 80）を出し、そこを押すとファイルを引っ張り始める。別のスレッドから
// CGEvent でその窓を押し、画面の (X, Y)（左上が原点）まで動かして放す。
// 使い方: mac-drag-source <ファイル> <X> <Y>

import AppKit

func say(_ s: String) {
    print("\(Date().timeIntervalSince1970) \(s)")
    fflush(stdout)
}

final class SourceView: NSView, NSDraggingSource {
    let file: URL

    init(frame: NSRect, file: URL) {
        self.file = file
        super.init(frame: frame)
    }

    required init?(coder: NSCoder) {
        fatalError("not used")
    }

    override func draw(_ dirtyRect: NSRect) {
        NSColor.systemBlue.setFill()
        bounds.fill()
    }

    override func mouseDown(with event: NSEvent) {
        say("source mouseDown")
        let item = NSDraggingItem(pasteboardWriter: file as NSURL)
        item.setDraggingFrame(bounds, contents: nil)
        beginDraggingSession(with: [item], event: event, source: self)
    }

    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        .copy
    }

    func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        say("source ended operation=\(operation.rawValue)")
    }
}

let args = CommandLine.arguments
guard args.count >= 4, let tx = Double(args[2]), let ty = Double(args[3]) else {
    say("usage: mac-drag-source <file> <x> <y>")
    exit(2)
}
let file = URL(fileURLWithPath: args[1])
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let screenHeight = NSScreen.main?.frame.height ?? 768
let win = NSWindow(contentRect: NSRect(x: 40, y: screenHeight - 80 - 80, width: 120, height: 80), styleMask: [.borderless], backing: .buffered, defer: false)
win.contentView = SourceView(frame: NSRect(x: 0, y: 0, width: 120, height: 80), file: file)
win.level = .floating
win.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
say("source window shown file=\(file.path)")

Thread {
    sleep(1)
    let source = CGEventSource(stateID: .hidSystemState)
    func mouse(_ type: CGEventType, _ x: Double, _ y: Double) {
        CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: CGPoint(x: x, y: y), mouseButton: .left)?.post(tap: .cghidEventTap)
    }
    // 元の窓の真ん中（左上が原点の画面の座標で x 40〜160、y 80〜160）
    let (sx, sy) = (100.0, 120.0)
    mouse(.mouseMoved, sx, sy)
    usleep(200_000)
    mouse(.leftMouseDown, sx, sy)
    usleep(200_000)
    for i in 1...40 {
        let t = Double(i) / 40
        mouse(.leftMouseDragged, sx + (tx - sx) * t, sy + (ty - sy) * t)
        usleep(30_000)
    }
    usleep(600_000)
    mouse(.leftMouseUp, tx, ty)
    say("released at \(tx),\(ty)")
    sleep(2)
    DispatchQueue.main.async {
        app.terminate(nil)
    }
}.start()
app.run()
