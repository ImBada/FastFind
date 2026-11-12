// The FastFind app icon, drawn as vector art at each pixel size: `render_icon <size> <out.png>`.
// Designed in a 1024-point space on the macOS icon grid (an 824-point body inset 100 points).
// `scripts/make_icon.sh` renders every size of `assets/AppIcon.iconset` with it.
import AppKit
import CoreGraphics

let args = CommandLine.arguments
let size = CGFloat(Double(args[1])!)
let out = args[2]
let detail = size >= 64

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

let space = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
ctx.interpolationQuality = .high
ctx.setShouldAntialias(true)
// Top-left origin, 1024 units across.
ctx.translateBy(x: 0, y: size)
ctx.scaleBy(x: size / 1024, y: -size / 1024)

func linear(_ colors: [CGColor], _ from: CGPoint, _ to: CGPoint, locations: [CGFloat]? = nil) {
    let gradient = CGGradient(colorsSpace: space, colors: colors as CFArray, locations: locations)!
    ctx.drawLinearGradient(gradient, start: from, end: to, options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
}

func radial(_ colors: [CGColor], _ center: CGPoint, _ radius: CGFloat, locations: [CGFloat]? = nil) {
    let gradient = CGGradient(colorsSpace: space, colors: colors as CFArray, locations: locations)!
    ctx.drawRadialGradient(gradient, startCenter: center, startRadius: 0, endCenter: center, endRadius: radius,
                           options: [.drawsAfterEndLocation])
}

/// Shadows ignore the CTM: their offset and blur are given in design units and scaled here.
func shadow(dy: CGFloat, blur: CGFloat, _ color: CGColor) {
    let k = size / 1024
    ctx.setShadow(offset: CGSize(width: 0, height: -dy * k), blur: blur * k, color: color)
}

func capsule(_ rect: CGRect) -> CGPath {
    CGPath(roundedRect: rect, cornerWidth: rect.height / 2, cornerHeight: rect.height / 2, transform: nil)
}

let body = CGRect(x: 100, y: 100, width: 824, height: 824)
// Measured against macOS 26's own icons: corners of about 0.233 of the body.
let shape = CGPath(roundedRect: body, cornerWidth: 192, cornerHeight: 192, transform: nil)

// Shadow under the body.
ctx.saveGState()
shadow(dy: 14, blur: 28, rgb(0x000000, 0.32))
ctx.addPath(shape)
ctx.setFillColor(rgb(0x3D3AD8))
ctx.fillPath()
ctx.restoreGState()

// Body: deep indigo into electric blue, lit from the top left.
ctx.saveGState()
ctx.addPath(shape)
ctx.clip()
linear([rgb(0x7A5CFF), rgb(0x4B48F0), rgb(0x1F6BFF)], CGPoint(x: 160, y: 100), CGPoint(x: 860, y: 924),
       locations: [0, 0.5, 1])
radial([rgb(0xFFFFFF, 0.28), rgb(0xFFFFFF, 0)], CGPoint(x: 300, y: 170), 560)
// Glass sheen across the top.
linear([rgb(0xFFFFFF, 0.20), rgb(0xFFFFFF, 0)], CGPoint(x: 0, y: 100), CGPoint(x: 0, y: 420))
ctx.restoreGState()

// Thin glass edge.
ctx.saveGState()
ctx.addPath(shape)
ctx.setStrokeColor(rgb(0xFFFFFF, 0.28))
ctx.setLineWidth(3)
ctx.addPath(shape)
ctx.clip()
ctx.addPath(shape)
ctx.strokePath()
ctx.restoreGState()

let lens = CGPoint(x: 476, y: 432)
let outer: CGFloat = 214
let rim: CGFloat = 46
let inner = outer - rim

// Speed lines, trailing to the left of the lens.
if detail {
    let streaks: [(y: CGFloat, x0: CGFloat, x1: CGFloat, h: CGFloat, a: CGFloat)] = [
        (lens.y - 96, 176, 262, 24, 0.55),
        (lens.y - 4, 136, 250, 30, 0.85),
        (lens.y + 88, 186, 268, 24, 0.45),
    ]
    for s in streaks {
        ctx.saveGState()
        ctx.addPath(capsule(CGRect(x: s.x0, y: s.y - s.h / 2, width: s.x1 - s.x0, height: s.h)))
        ctx.clip()
        linear([rgb(0xFFFFFF, 0), rgb(0xFFFFFF, s.a)], CGPoint(x: s.x0, y: 0), CGPoint(x: s.x1, y: 0))
        ctx.restoreGState()
    }
}

// Handle, at 45° from the lens, with a darker collar where it meets the rim.
let dir = CGPoint(x: cos(CGFloat.pi / 4), y: sin(CGFloat.pi / 4))
let handleStart = CGPoint(x: lens.x + dir.x * (outer - 12), y: lens.y + dir.y * (outer - 12))
let handleEnd = CGPoint(x: lens.x + dir.x * (outer + 236), y: lens.y + dir.y * (outer + 236))
ctx.saveGState()
shadow(dy: 16, blur: 30, rgb(0x0B0B4A, 0.45))
ctx.setLineCap(.round)
ctx.setLineWidth(84)
ctx.setStrokeColor(rgb(0xE9ECFF))
ctx.move(to: handleStart)
ctx.addLine(to: handleEnd)
ctx.strokePath()
ctx.restoreGState()
ctx.saveGState()
ctx.setLineCap(.round)
ctx.setLineWidth(84)
ctx.move(to: handleStart)
ctx.addLine(to: handleEnd)
ctx.replacePathWithStrokedPath()
ctx.clip()
linear([rgb(0xFFFFFF), rgb(0xC7CEFF)], CGPoint(x: handleStart.x - 40, y: handleStart.y + 40),
       CGPoint(x: handleStart.x + 40, y: handleStart.y - 40))
// Collar.
ctx.setFillColor(rgb(0xFFD60A))
let collarAt = CGPoint(x: lens.x + dir.x * (outer + 40), y: lens.y + dir.y * (outer + 40))
ctx.translateBy(x: collarAt.x, y: collarAt.y)
ctx.rotate(by: .pi / 4)
ctx.fill(CGRect(x: -26, y: -60, width: 52, height: 120))
ctx.restoreGState()

// Lens: shadow, frosted glass, the result rows seen through it, then the rim.
ctx.saveGState()
shadow(dy: 14, blur: 34, rgb(0x0B0B4A, 0.40))
ctx.addEllipse(in: CGRect(x: lens.x - outer, y: lens.y - outer, width: outer * 2, height: outer * 2))
ctx.setFillColor(rgb(0x5A55F5))
ctx.fillPath()
ctx.restoreGState()

ctx.saveGState()
ctx.addEllipse(in: CGRect(x: lens.x - inner, y: lens.y - inner, width: inner * 2, height: inner * 2))
ctx.clip()
linear([rgb(0xFFFFFF, 0.30), rgb(0xFFFFFF, 0.10)], CGPoint(x: 0, y: lens.y - inner), CGPoint(x: 0, y: lens.y + inner))
if detail {
    let rows: [(dy: CGFloat, length: CGFloat, match: Bool)] = [(-74, 190, false), (0, 230, true), (74, 160, false)]
    for row in rows {
        let y = lens.y + row.dy
        let x0 = lens.x - 128
        // The item's icon dot.
        ctx.setFillColor(row.match ? rgb(0xFF5C8A) : rgb(0xFFFFFF, 0.75))
        ctx.fillEllipse(in: CGRect(x: x0, y: y - 18, width: 36, height: 36))
        // Its name; the match is highlighted, as in the results list.
        ctx.addPath(capsule(CGRect(x: x0 + 52, y: y - 15, width: row.length - 52, height: 30)))
        ctx.setFillColor(row.match ? rgb(0xFFD60A) : rgb(0xFFFFFF, 0.6))
        ctx.fillPath()
    }
} else {
    ctx.addPath(capsule(CGRect(x: lens.x - 110, y: lens.y - 24, width: 220, height: 48)))
    ctx.setFillColor(rgb(0xFFD60A))
    ctx.fillPath()
}
// A soft reflection across the upper glass.
radial([rgb(0xFFFFFF, 0.35), rgb(0xFFFFFF, 0)], CGPoint(x: lens.x - 70, y: lens.y - 90), 150)
ctx.restoreGState()

// Rim.
let ring = CGMutablePath()
ring.addEllipse(in: CGRect(x: lens.x - outer, y: lens.y - outer, width: outer * 2, height: outer * 2))
ring.addEllipse(in: CGRect(x: lens.x - inner, y: lens.y - inner, width: inner * 2, height: inner * 2))
ctx.saveGState()
ctx.addPath(ring)
ctx.clip(using: .evenOdd)
linear([rgb(0xFFFFFF), rgb(0xD9DEFF), rgb(0xAEB8FF)], CGPoint(x: lens.x - outer, y: lens.y - outer),
       CGPoint(x: lens.x + outer, y: lens.y + outer), locations: [0, 0.55, 1])
ctx.restoreGState()
// Specular arc on the rim's top left.
ctx.saveGState()
ctx.setLineCap(.round)
ctx.setLineWidth(12)
ctx.setStrokeColor(rgb(0xFFFFFF, 0.95))
ctx.addArc(center: lens, radius: outer - rim / 2, startAngle: .pi * 1.08, endAngle: .pi * 1.42, clockwise: false)
ctx.strokePath()
ctx.restoreGState()

let image = ctx.makeImage()!
let rep = NSBitmapImageRep(cgImage: image)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
