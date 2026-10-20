package io.solidcraft;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;

/**
 * SolidCraft's one Activity: Android's {@link NativeActivity} plus the answer of a Storage
 * Access Framework picker.
 *
 * <p>The system answers {@code startActivityForResult} through {@link #onActivityResult}, a
 * callback {@link NativeActivity} has no use for and never receives. This subclass catches it
 * and hands the chosen document to the Rust side through {@link #nativeResult}, whose symbol
 * the cdylib exports and which the shell registers before any picker opens. Without this
 * class the pickers' answers would simply vanish.
 */
public final class SolidcraftActivity extends NativeActivity {

    /**
     * The cdylib's {@code native_result} (src/saf.rs): the latest picker answer. {@code uri} is
     * null when the system returned no document. The JNI signature must stay in step with the
     * {@code native_method!} declaration there.
     */
    private static native void nativeResult(int requestCode, int resultCode, String uri);

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        Uri uri = data == null ? null : data.getData();
        nativeResult(requestCode, resultCode, uri == null ? null : uri.toString());
    }
}
