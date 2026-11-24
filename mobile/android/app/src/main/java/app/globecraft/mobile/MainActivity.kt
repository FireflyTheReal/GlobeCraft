package app.globecraft.mobile

import android.app.Activity
import android.graphics.Color
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.WindowInsets
import android.widget.FrameLayout
import android.widget.Switch
import android.widget.TextView

/**
 * Phase 0: the globe full screen, a statistics line (renderer, frame size and time,
 * downloads) and a Relief switch, to measure the renderer on the device. The app's
 * real screens (search, layers, details in a bottom sheet) come in Phase 2.
 */
class MainActivity : Activity() {
    private var handle = 0L
    private var globe: GlobeView? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setDecorFitsSystemWindows(false)
        handle = GlobeNative.create(filesDir.absolutePath)
        val root = FrameLayout(this)
        if (handle == 0L) {
            root.addView(TextView(this).apply {
                text = "The globe could not start. See logcat (tag GlobeCraft)."
                setTextColor(Color.WHITE)
                gravity = Gravity.CENTER
            })
            setContentView(root)
            return
        }
        val view = GlobeView(this, handle)
        globe = view
        root.addView(view)

        val stats = TextView(this).apply {
            setTextColor(Color.WHITE)
            setBackgroundColor(Color.argb(140, 0, 0, 0))
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setPadding(16, 8, 16, 8)
        }
        view.onStats = { stats.text = it }
        root.addView(stats, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.TOP or Gravity.START,
        ))

        val relief = Switch(this).apply {
            text = getString(R.string.relief)
            isChecked = true
            setTextColor(Color.WHITE)
            setBackgroundColor(Color.argb(140, 0, 0, 0))
            setPadding(24, 12, 24, 12)
            minHeight = (48 * resources.displayMetrics.density).toInt()
            setOnCheckedChangeListener { _, on ->
                GlobeNative.setRelief(handle, on)
                view.resume()
            }
        }
        root.addView(relief, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT,
            Gravity.BOTTOM or Gravity.END,
        ))

        // Keep the overlays clear of the status bar, the cut-out and the gesture bar.
        root.setOnApplyWindowInsetsListener { _, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            (stats.layoutParams as FrameLayout.LayoutParams).setMargins(bars.left + 16, bars.top + 16, 0, 0)
            (relief.layoutParams as FrameLayout.LayoutParams).setMargins(0, 0, bars.right + 16, bars.bottom + 16)
            stats.requestLayout(); relief.requestLayout()
            insets
        }
        setContentView(root)
    }

    override fun onPause() {
        super.onPause()
        globe?.pause()
    }

    override fun onResume() {
        super.onResume()
        globe?.resume()
    }

    override fun onDestroy() {
        globe = null
        if (handle != 0L) {
            GlobeNative.destroy(handle)
            handle = 0L
        }
        super.onDestroy()
    }
}
