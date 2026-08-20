#!/usr/bin/env swift

import AppKit
import Foundation

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("render-svg: \(message)\n".utf8))
    exit(2)
}

let arguments = CommandLine.arguments
guard arguments.count == 4 || arguments.count == 5 else {
    fail("usage: render-svg <input.svg> <output.png> <width> [height]")
}

let inputURL = URL(fileURLWithPath: arguments[1])
let outputURL = URL(fileURLWithPath: arguments[2])
guard let width = Int(arguments[3]), width > 0 else {
    fail("width must be a positive integer")
}
let height: Int
if arguments.count == 5 {
    guard let parsedHeight = Int(arguments[4]), parsedHeight > 0 else {
        fail("height must be a positive integer")
    }
    height = parsedHeight
} else {
    height = width
}

guard let image = NSImage(contentsOf: inputURL) else {
    fail("cannot load \(inputURL.path)")
}
guard let bitmap = NSBitmapImageRep(
    bitmapDataPlanes: nil,
    pixelsWide: width,
    pixelsHigh: height,
    bitsPerSample: 8,
    samplesPerPixel: 4,
    hasAlpha: true,
    isPlanar: false,
    colorSpaceName: .deviceRGB,
    bitmapFormat: [],
    bytesPerRow: 0,
    bitsPerPixel: 0
) else {
    fail("cannot allocate \(width)x\(height) RGBA bitmap")
}
guard let graphicsContext = NSGraphicsContext(bitmapImageRep: bitmap) else {
    fail("cannot create bitmap graphics context")
}

NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = graphicsContext
graphicsContext.imageInterpolation = .high
graphicsContext.cgContext.clear(CGRect(x: 0, y: 0, width: width, height: height))
image.draw(
    in: NSRect(x: 0, y: 0, width: width, height: height),
    from: .zero,
    operation: .sourceOver,
    fraction: 1
)
graphicsContext.flushGraphics()
NSGraphicsContext.restoreGraphicsState()

guard let png = bitmap.representation(using: .png, properties: [:]) else {
    fail("cannot encode PNG")
}

do {
    try png.write(to: outputURL, options: .atomic)
} catch {
    fail("cannot write \(outputURL.path): \(error.localizedDescription)")
}
