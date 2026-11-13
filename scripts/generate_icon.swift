#!/usr/bin/env swift
import AppKit
import Foundation

guard CommandLine.arguments.count >= 2 else {
    fputs("Usage: generate_icon.swift <output_iconset_dir>\n", stderr)
    exit(1)
}

let outDir = URL(fileURLWithPath: CommandLine.arguments[1])
try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

func renderIcon(pixels: Int) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: pixels,
        pixelsHigh: pixels,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    )!
    rep.size = NSSize(width: pixels, height: pixels)

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let ctx = NSGraphicsContext.current!.cgContext
    let s = CGFloat(pixels)

    // Clear background
    ctx.clear(CGRect(x: 0, y: 0, width: s, height: s))

    // macOS rounded-square squircle inset (~82% canvas)
    let inset = s * 0.088
    let rect = CGRect(x: inset, y: inset, width: s - inset * 2, height: s - inset * 2)
    let corner = rect.width * 0.225
    let squircle = CGPath(roundedRect: rect, cornerWidth: corner, cornerHeight: corner, transform: nil)

    ctx.saveGState()
    ctx.addPath(squircle)
    ctx.clip()

    let cs = CGColorSpaceCreateDeviceRGB()

    // Deep teal-slate ocean gradient background
    let bgGrad = CGGradient(
        colorsSpace: cs,
        colors: [
            CGColor(red: 0.06, green: 0.14, blue: 0.20, alpha: 1.0),
            CGColor(red: 0.04, green: 0.24, blue: 0.30, alpha: 1.0),
            CGColor(red: 0.03, green: 0.10, blue: 0.16, alpha: 1.0),
        ] as CFArray,
        locations: [0.0, 0.52, 1.0]
    )!
    ctx.drawLinearGradient(
        bgGrad,
        start: CGPoint(x: rect.minX, y: rect.maxY),
        end: CGPoint(x: rect.maxX, y: rect.minY),
        options: []
    )

    // Subtle luminous halo behind pearl
    let center = CGPoint(x: s * 0.5, y: s * 0.5)
    let haloGrad = CGGradient(
        colorsSpace: cs,
        colors: [
            CGColor(red: 0.42, green: 0.88, blue: 0.86, alpha: 0.42),
            CGColor(red: 0.18, green: 0.62, blue: 0.72, alpha: 0.12),
            CGColor(red: 0.05, green: 0.18, blue: 0.24, alpha: 0.0),
        ] as CFArray,
        locations: [0.0, 0.5, 1.0]
    )!
    ctx.drawRadialGradient(
        haloGrad,
        startCenter: center,
        startRadius: s * 0.12,
        endCenter: center,
        endRadius: s * 0.46,
        options: []
    )

    // Pearl orb shadow
    let orbRadius = s * 0.255
    let orbRect = CGRect(
        x: center.x - orbRadius,
        y: center.y - orbRadius,
        width: orbRadius * 2,
        height: orbRadius * 2
    )
    ctx.saveGState()
    ctx.setShadow(
        offset: CGSize(width: 0, height: -s * 0.025),
        blur: s * 0.06,
        color: CGColor(red: 0.0, green: 0.0, blue: 0.0, alpha: 0.55)
    )
    ctx.setFillColor(CGColor(red: 0.85, green: 0.94, blue: 0.96, alpha: 1.0))
    ctx.fillEllipse(in: orbRect)
    ctx.restoreGState()

    // Iridescent Pearl shading
    ctx.saveGState()
    ctx.addEllipse(in: orbRect)
    ctx.clip()

    let pearlGrad = CGGradient(
        colorsSpace: cs,
        colors: [
            CGColor(red: 1.00, green: 1.00, blue: 1.00, alpha: 1.0),
            CGColor(red: 0.88, green: 0.96, blue: 0.97, alpha: 1.0),
            CGColor(red: 0.62, green: 0.84, blue: 0.88, alpha: 1.0),
            CGColor(red: 0.28, green: 0.55, blue: 0.64, alpha: 1.0),
        ] as CFArray,
        locations: [0.0, 0.30, 0.72, 1.0]
    )!
    let highlightPt = CGPoint(x: center.x - orbRadius * 0.34, y: center.y + orbRadius * 0.38)
    ctx.drawRadialGradient(
        pearlGrad,
        startCenter: highlightPt,
        startRadius: orbRadius * 0.04,
        endCenter: center,
        endRadius: orbRadius * 1.15,
        options: [.drawsAfterEndLocation]
    )

    // Soft rose-teal nacre rim reflection
    let rimGrad = CGGradient(
        colorsSpace: cs,
        colors: [
            CGColor(red: 0.55, green: 0.95, blue: 0.92, alpha: 0.0),
            CGColor(red: 0.68, green: 0.92, blue: 0.96, alpha: 0.28),
        ] as CFArray,
        locations: [0.68, 1.0]
    )!
    ctx.drawRadialGradient(
        rimGrad,
        startCenter: CGPoint(x: center.x - orbRadius * 0.1, y: center.y + orbRadius * 0.1),
        startRadius: orbRadius * 0.5,
        endCenter: center,
        endRadius: orbRadius,
        options: []
    )

    // Crisp specular pinpoint
    let specRect = CGRect(
        x: center.x - orbRadius * 0.44,
        y: center.y + orbRadius * 0.24,
        width: orbRadius * 0.26,
        height: orbRadius * 0.18
    )
    ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 0.82))
    ctx.fillEllipse(in: specRect)
    ctx.restoreGState()

    // Subtle inner rim on squircle
    ctx.addPath(squircle)
    ctx.setStrokeColor(CGColor(red: 1.0, green: 1.0, blue: 1.0, alpha: 0.16))
    ctx.setLineWidth(max(1.0, s * 0.008))
    ctx.strokePath()

    ctx.restoreGState()
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

let specs: [(String, Int)] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
]

for (filename, size) in specs {
    let rep = renderIcon(pixels: size)
    let data = rep.representation(using: .png, properties: [:])!
    try data.write(to: outDir.appendingPathComponent(filename))
}
