import SwiftUI
import UIKit

struct GlobeRepresentable: UIViewRepresentable {
    let globe: OpaquePointer
    @Binding var stats: String

    func makeUIView(context: Context) -> GlobeUIView {
        let view = GlobeUIView(globe: globe)
        view.onStats = { text in
            DispatchQueue.main.async { stats = text }
        }
        return view
    }

    func updateUIView(_ view: GlobeUIView, context: Context) {
        // Settings (Relief) change through the model; make sure a frame follows.
        view.start()
    }
}

/// The globe, drawn by Rust and shown as the layer's contents. One finger turns the
/// globe; two fingers pinch to zoom and twist to rotate, or drag up and down together
/// to tilt. Double-tap zooms in, a two-finger tap zooms out. Frames are asked for once
/// per display refresh, and only while the globe has work.
final class GlobeUIView: UIView, UIGestureRecognizerDelegate {
    let globe: OpaquePointer
    var onStats: ((String) -> Void)?

    private var link: CADisplayLink?
    private var lastStats: CFTimeInterval = 0
    private var statsBuffer = [CChar](repeating: 0, count: 256)

    private let onePan = UIPanGestureRecognizer()
    private let twoPan = UIPanGestureRecognizer()
    private let pinch = UIPinchGestureRecognizer()
    private let rotation = UIRotationGestureRecognizer()
    /// The two-finger gesture under way: undecided until it moves enough.
    private enum TwoFinger { case undecided, zoomRotate, tilt }
    private var twoFinger = TwoFinger.undecided
    private var pinchSinceStart: CGFloat = 1
    private var turnSinceStart: CGFloat = 0
    private var moveSinceStart = CGPoint.zero
    private var lastMove = CGPoint.zero

    init(globe: OpaquePointer) {
        self.globe = globe
        super.init(frame: .zero)
        backgroundColor = .black
        isMultipleTouchEnabled = true
        layer.contentsGravity = .resize
        layer.magnificationFilter = .linear
        isAccessibilityElement = true
        accessibilityLabel = "Globe"
        accessibilityHint = "Drag to turn, pinch to zoom, twist to rotate, drag up or down with two fingers to tilt."
        accessibilityTraits = .allowsDirectInteraction

        onePan.maximumNumberOfTouches = 1
        onePan.addTarget(self, action: #selector(handleOnePan))
        twoPan.minimumNumberOfTouches = 2
        twoPan.maximumNumberOfTouches = 2
        twoPan.addTarget(self, action: #selector(handleTwoPan))
        pinch.addTarget(self, action: #selector(handlePinch))
        rotation.addTarget(self, action: #selector(handleRotation))
        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(handleDoubleTap))
        doubleTap.numberOfTapsRequired = 2
        let twoFingerTap = UITapGestureRecognizer(target: self, action: #selector(handleTwoFingerTap))
        twoFingerTap.numberOfTouchesRequired = 2
        for recognizer in [onePan, twoPan, pinch, rotation, doubleTap, twoFingerTap] {
            recognizer.delegate = self
            addGestureRecognizer(recognizer)
        }
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer,
                           shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
        true
    }

    // MARK: Size and frames

    override func layoutSubviews() {
        super.layoutSubviews()
        let scale = traitCollection.displayScale
        gc_set_size(globe, UInt32(bounds.width * scale), UInt32(bounds.height * scale), Float(scale))
        start()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window == nil { stop() } else { start() }
    }

    func start() {
        guard link == nil, window != nil else { return }
        let link = CADisplayLink(target: self, selector: #selector(step))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    private func stop() {
        link?.invalidate()
        link = nil
    }

    private var gesturing: Bool {
        [onePan, twoPan, pinch, rotation].contains { $0.state == .began || $0.state == .changed }
    }

    @objc private func step(_ link: CADisplayLink) {
        if gc_tick(globe) { show() }
        if link.timestamp - lastStats > 0.5 {
            lastStats = link.timestamp
            let count = statsBuffer.count
            gc_stats(globe, &statsBuffer, count)
            onStats?(String(cString: statsBuffer))
        }
        if !gc_busy(globe) && !gesturing { stop() }
    }

    private func show() {
        var width: UInt32 = 0
        var height: UInt32 = 0
        guard let pixels = gc_frame(globe, &width, &height), width > 0, height > 0 else { return }
        let bytesPerRow = Int(width) * 4
        // Copied: the frame's memory is reused by the next tick.
        guard let data = CFDataCreate(nil, pixels, bytesPerRow * Int(height)),
              let provider = CGDataProvider(data: data),
              let image = CGImage(
                  width: Int(width), height: Int(height),
                  bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: bytesPerRow,
                  space: CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
        else { return }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.contents = image
        CATransaction.commit()
    }

    // MARK: Gestures (points)

    @objc private func handleOnePan(_ g: UIPanGestureRecognizer) {
        let t = g.translation(in: self)
        gc_pan(globe, Float(t.x), Float(t.y))
        g.setTranslation(.zero, in: self)
        start()
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        super.touchesBegan(touches, with: event)
        if (event?.allTouches?.count ?? 0) >= 2 {
            twoFinger = .undecided
            pinchSinceStart = 1
            turnSinceStart = 0
            moveSinceStart = .zero
            lastMove = .zero
        }
    }

    /// Settles what a two-finger gesture is, once it moved enough: fingers going up or
    /// down together tilt; spreading or twisting zooms and rotates. What moved before the
    /// decision is applied when it falls, so nothing is lost.
    private func decide() {
        guard twoFinger == .undecided else { return }
        let rise = abs(moveSinceStart.y)
        let stretch = abs(pinchSinceStart - 1)
        if rise > 20, abs(moveSinceStart.x) < rise * 0.5, stretch < 0.08, abs(turnSinceStart) < 0.15 {
            twoFinger = .tilt
            gc_tilt(globe, Float(-moveSinceStart.y * 0.3))
        } else if stretch > 0.08 || abs(turnSinceStart) > 0.1 {
            twoFinger = .zoomRotate
            let at = pinch.numberOfTouches >= 2 ? pinch.location(in: self) : CGPoint(x: bounds.midX, y: bounds.midY)
            gc_pinch(globe, Float(pinchSinceStart), Float(at.x), Float(at.y))
            gc_rotate(globe, Float(turnSinceStart * 180 / .pi))
            gc_pan(globe, Float(moveSinceStart.x), Float(moveSinceStart.y))
        }
    }

    @objc private func handleTwoPan(_ g: UIPanGestureRecognizer) {
        switch g.state {
        case .began, .changed:
            let total = g.translation(in: self)
            let dx = total.x - lastMove.x
            let dy = total.y - lastMove.y
            lastMove = total
            moveSinceStart = total
            switch twoFinger {
            // Fingers moving up lean the view toward the horizon.
            case .tilt: gc_tilt(globe, Float(-dy * 0.3))
            case .zoomRotate: gc_pan(globe, Float(dx), Float(dy))
            case .undecided: decide()
            }
        default:
            lastMove = .zero
        }
        start()
    }

    @objc private func handlePinch(_ g: UIPinchGestureRecognizer) {
        switch twoFinger {
        case .undecided:
            pinchSinceStart *= g.scale
            decide()
        case .zoomRotate where g.numberOfTouches >= 2:
            let at = g.location(in: self)
            gc_pinch(globe, Float(g.scale), Float(at.x), Float(at.y))
        default:
            break
        }
        g.scale = 1
        start()
    }

    @objc private func handleRotation(_ g: UIRotationGestureRecognizer) {
        switch twoFinger {
        case .undecided:
            turnSinceStart += g.rotation
            decide()
        case .zoomRotate:
            // UIKit's rotation is clockwise-positive, in radians.
            gc_rotate(globe, Float(g.rotation * 180 / .pi))
        case .tilt:
            break
        }
        g.rotation = 0
        start()
    }

    @objc private func handleDoubleTap(_ g: UITapGestureRecognizer) {
        let at = g.location(in: self)
        gc_pinch(globe, 2, Float(at.x), Float(at.y))
        start()
    }

    @objc private func handleTwoFingerTap(_ g: UITapGestureRecognizer) {
        let at = g.location(in: self)
        gc_pinch(globe, 0.5, Float(at.x), Float(at.y))
        start()
    }
}
