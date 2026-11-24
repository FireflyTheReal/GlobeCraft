package app.globecraft.mobile

/** The Rust globe (crates/globecraft-mobile/src/android.rs). Gestures are in dp. */
object GlobeNative {
    init {
        System.loadLibrary("globecraft_mobile")
    }

    /**
     * Starts the globe with its caches below [appDir]; 0 if it could not start. [context]
     * lets the Rust side check server certificates with Android's own verifier.
     */
    external fun create(context: android.content.Context, appDir: String): Long
    external fun destroy(handle: Long)
    /** The Surface to draw into, or null when it is gone. */
    external fun setSurface(handle: Long, surface: android.view.Surface?)
    external fun setSize(handle: Long, width: Int, height: Int, density: Float)
    external fun pan(handle: Long, dx: Float, dy: Float)
    external fun pinch(handle: Long, scale: Float, x: Float, y: Float)
    external fun rotate(handle: Long, degrees: Float)
    external fun tilt(handle: Long, degrees: Float)
    external fun setRelief(handle: Long, on: Boolean)
    /** One display refresh; returns whether to keep ticking. */
    external fun tick(handle: Long): Boolean
    external fun stats(handle: Long): String?
}
