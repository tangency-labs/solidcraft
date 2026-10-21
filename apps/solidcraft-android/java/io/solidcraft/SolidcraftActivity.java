package io.solidcraft;

import android.app.NativeActivity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

/**
 * SolidCraft's one Activity: Android's {@link NativeActivity} plus the answers of a Storage
 * Access Framework picker and of the "Open with" intents the file browsers send.
 *
 * <p>The system answers {@code startActivityForResult} through {@link #onActivityResult}, a
 * callback {@link NativeActivity} has no use for and never receives. This subclass catches it
 * and hands the chosen document to the Rust side through {@link #nativeResult}, whose symbol
 * the cdylib exports and which the shell registers before any picker opens. Without this
 * class the pickers' answers would simply vanish.
 *
 * <p>A document the user opens from the file browser arrives differently: as an
 * {@code ACTION_VIEW} intent whose data is a {@code content://} uri, which the Rust side
 * cannot read as a path. The Activity leaves it in {@link #pendingView}; the shell reads it
 * there and copies the document into the app's own store (src/saf.rs).
 */
public final class SolidcraftActivity extends NativeActivity {

    /**
     * The cdylib's {@code native_result} (src/saf.rs): the latest picker answer. {@code uri} is
     * null when the system returned no document. The JNI signature must stay in step with the
     * {@code native_method!} declaration there.
     */
    private static native void nativeResult(int requestCode, int resultCode, String uri);

    /**
     * The document an {@code ACTION_VIEW} intent asked us to open, as {@code count|type|uri}:
     * the count tells the Rust side that this is a new request, even when it is the same
     * document twice, and the type is the one the browser sent (empty when it sent none) —
     * it names the format of a document whose own name carries no extension. Written on the
     * Java main thread and read from the render thread, hence {@code volatile}. The field name
     * must stay in step with {@code pending_view} in src/saf.rs, which the manifest test
     * checks.
     */
    private static volatile String pendingView;

    /**
     * How many {@code ACTION_VIEW} intents arrived: the number in {@link #pendingView}.
     */
    private static long views;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        viewed(getIntent());
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        // The Activity is `singleTop`: a second document arrives here, not in a new Activity.
        setIntent(intent);
        viewed(intent);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        Uri uri = data == null ? null : data.getData();
        nativeResult(requestCode, resultCode, uri == null ? null : uri.toString());
    }

    /**
     * Remember the document an {@code ACTION_VIEW} intent carries, if any. The count, the type
     * and the uri travel together, so the Rust side reads one consistent set.
     */
    private static void viewed(Intent intent) {
        if (intent == null || !Intent.ACTION_VIEW.equals(intent.getAction())) {
            return;
        }
        Uri data = intent.getData();
        if (data == null) {
            return;
        }
        views += 1;
        String type = intent.getType();
        pendingView = views + "|" + (type == null ? "" : type) + "|" + data.toString();
    }
}
