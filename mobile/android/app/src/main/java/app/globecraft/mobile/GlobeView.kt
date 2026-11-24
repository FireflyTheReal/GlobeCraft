package app.globecraft.mobile

import android.annotation.SuppressLint
import android.content.Context
import android.view.Choreographer
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.hypot

/**
 * The globe, drawn by Rust into this view's Surface. One finger turns the globe; two
 * fingers pinch to zoom and twist to rotate, or drag up and down together to tilt.
 * Double-tap zooms in, a two-finger tap zooms out. Frames are asked for once per
 * display refresh, and only while the globe has work (a gesture, a frame, downloads).
 */
@SuppressLint("ViewConstructor")
class GlobeView(context: Context, private val handle: Long) :
    SurfaceView(context), SurfaceHolder.Callback, Choreographer.FrameCallback {

    var onStats: ((String) -> Unit)? = null

    private val density = resources.displayMetrics.density
    private var ticking = false
    private var paused = false
    private var touching = false
    private var lastStats = 0L

    private enum class Mode { NONE, ONE, TWO_UNDECIDED, TWO_ZOOM_ROTATE, TWO_TILT }
    private var mode = Mode.NONE
    private var lastX = 0f
    private var lastY = 0f
    private var downX = 0f
    private var downY = 0f
    private var downTime = 0L
    private var lastTapTime = 0L
    private var lastTapX = 0f
    private var lastTapY = 0f
    private var lastSpan = 0f
    private var lastAngle = 0f
    private var lastCx = 0f
    private var lastCy = 0f
    private var startSpan = 0f
    private var startAngle = 0f
    private var startCx = 0f
    private var startCy = 0f
    private var startY0 = 0f
    private var startY1 = 0f
    private var twoDownTime = 0L

    init {
        holder.addCallback(this)
        contentDescription = context.getString(R.string.globe_description)
    }

    // --- Surface ---------------------------------------------------------------------

    override fun surfaceCreated(holder: SurfaceHolder) {
        GlobeNative.setSurface(handle, holder.surface)
        startTicking()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        GlobeNative.setSize(handle, width, height, density)
        startTicking()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        GlobeNative.setSurface(handle, null)
    }

    // --- Ticking ---------------------------------------------------------------------

    fun pause() {
        paused = true
    }

    fun resume() {
        paused = false
        startTicking()
    }

    private fun startTicking() {
        if (!ticking && !paused) {
            ticking = true
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (paused) {
            ticking = false
            return
        }
        val busy = GlobeNative.tick(handle)
        val now = System.currentTimeMillis()
        if (now - lastStats > 500) {
            lastStats = now
            GlobeNative.stats(handle)?.let { onStats?.invoke(it) }
        }
        if (busy || touching) {
            Choreographer.getInstance().postFrameCallback(this)
        } else {
            ticking = false
        }
    }

    // --- Gestures (all distances in dp) -----------------------------------------------

    private fun dp(px: Float) = px / density

    private fun twoFinger(event: MotionEvent): FloatArray {
        val x0 = event.getX(0); val y0 = event.getY(0)
        val x1 = event.getX(1); val y1 = event.getY(1)
        val angle = Math.toDegrees(atan2((y1 - y0).toDouble(), (x1 - x0).toDouble())).toFloat()
        return floatArrayOf(hypot(x1 - x0, y1 - y0), angle, (x0 + x1) / 2, (y0 + y1) / 2)
    }

    private fun turn(from: Float, to: Float): Float {
        var d = to - from
        while (d > 180) d -= 360
        while (d < -180) d += 360
        return d
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                touching = true
                mode = Mode.ONE
                lastX = event.x; lastY = event.y
                downX = event.x; downY = event.y
                downTime = event.eventTime
                startTicking()
            }
            MotionEvent.ACTION_POINTER_DOWN -> if (event.pointerCount == 2) {
                val (span, angle, cx, cy) = twoFinger(event)
                mode = Mode.TWO_UNDECIDED
                lastSpan = span; startSpan = span
                lastAngle = angle; startAngle = angle
                lastCx = cx; lastCy = cy; startCx = cx; startCy = cy
                startY0 = event.getY(0); startY1 = event.getY(1)
                twoDownTime = event.eventTime
            }
            MotionEvent.ACTION_MOVE -> when (mode) {
                Mode.ONE -> {
                    GlobeNative.pan(handle, dp(event.x - lastX), dp(event.y - lastY))
                    lastX = event.x; lastY = event.y
                }
                Mode.TWO_UNDECIDED, Mode.TWO_ZOOM_ROTATE, Mode.TWO_TILT -> if (event.pointerCount >= 2) {
                    val (span, angle, cx, cy) = twoFinger(event)
                    if (mode == Mode.TWO_UNDECIDED) {
                        // Until the gesture is decided nothing is applied; once it is,
                        // everything since the fingers came down is (nothing is lost).
                        lastSpan = startSpan; lastAngle = startAngle; lastCx = startCx; lastCy = startCy
                        val rise = dp(abs(cy - startCy))
                        val stretch = dp(abs(span - startSpan))
                        val twist = abs(turn(startAngle, angle))
                        val sameWay = (event.getY(0) - startY0) * (event.getY(1) - startY1) > 0
                        mode = when {
                            rise > 20 && stretch < rise * 0.5f && twist < 10 && sameWay -> Mode.TWO_TILT
                            stretch > 12 || twist > 6 -> Mode.TWO_ZOOM_ROTATE
                            else -> Mode.TWO_UNDECIDED
                        }
                    }
                    when (mode) {
                        Mode.TWO_ZOOM_ROTATE -> {
                            if (lastSpan > 0) GlobeNative.pinch(handle, span / lastSpan, dp(cx), dp(cy))
                            GlobeNative.rotate(handle, turn(lastAngle, angle))
                            GlobeNative.pan(handle, dp(cx - lastCx), dp(cy - lastCy))
                        }
                        // Fingers moving up lean the view toward the horizon.
                        Mode.TWO_TILT -> GlobeNative.tilt(handle, -dp(cy - lastCy) * 0.3f)
                        else -> {}
                    }
                    lastSpan = span; lastAngle = angle; lastCx = cx; lastCy = cy
                }
                Mode.NONE -> {}
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (mode == Mode.TWO_UNDECIDED && event.eventTime - twoDownTime < 250) {
                    GlobeNative.pinch(handle, 0.5f, dp(lastCx), dp(lastCy))
                }
                // Carry on with the finger that stays, from where it is.
                val stays = if (event.actionIndex == 0) 1 else 0
                if (event.pointerCount == 2) {
                    mode = Mode.ONE
                    lastX = event.getX(stays); lastY = event.getY(stays)
                    downTime = 0 // not a tap any more
                }
            }
            MotionEvent.ACTION_UP -> {
                touching = false
                val tap = mode == Mode.ONE && downTime != 0L &&
                    event.eventTime - downTime < 250 &&
                    dp(hypot(event.x - downX, event.y - downY)) < 8
                if (tap) {
                    if (event.eventTime - lastTapTime < 300 && dp(hypot(event.x - lastTapX, event.y - lastTapY)) < 30) {
                        GlobeNative.pinch(handle, 2f, dp(event.x), dp(event.y))
                        lastTapTime = 0
                    } else {
                        lastTapTime = event.eventTime; lastTapX = event.x; lastTapY = event.y
                    }
                }
                mode = Mode.NONE
                startTicking()
            }
            MotionEvent.ACTION_CANCEL -> {
                touching = false
                mode = Mode.NONE
            }
        }
        return true
    }
}
