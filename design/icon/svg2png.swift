import AppKit
// svg → png：走系统自带的 SVG 渲染（NSImage），不装任何东西。make_icon.py 用它画兜底的 .icns
// 用法：svg2png <in.svg> <out.png> <边长像素>
let a = CommandLine.arguments
guard a.count == 4, let px = Int(a[3]) else { fatalError("用法：svg2png <in.svg> <out.png> <边长像素>") }
guard let img = NSImage(contentsOf: URL(fileURLWithPath: a[1])) else { fatalError("读不了 \(a[1])") }
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSGraphicsContext.current!.imageInterpolation = .high
img.draw(in: NSRect(x: 0, y: 0, width: px, height: px))
NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[2]))
