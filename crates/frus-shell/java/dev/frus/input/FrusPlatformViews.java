package dev.frus.input;

import android.app.Activity;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.PixelFormat;
import android.graphics.PorterDuff;
import android.graphics.Rect;
import android.hardware.HardwareBuffer;
import android.media.Image;
import android.media.ImageReader;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.text.Editable;
import android.text.TextWatcher;
import android.view.Choreographer;
import android.view.InputDevice;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.Surface;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.ViewTreeObserver;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.TextView;
import dalvik.system.InMemoryDexClassLoader;
import dev.frus.platform.PlatformView;
import dev.frus.platform.PlatformViewChannel;
import dev.frus.platform.PlatformViewFactory;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Native views inside a frus window (milestone 794).
 *
 * <p>A frus window's surface belongs to the native side: the views of the activity are laid
 * out but never drawn. So each embedded view sits in the activity's hierarchy -- where focus,
 * the keyboard and the screen reader treat it as any other view -- inside a {@link Host} that
 * draws it, whenever something in it changes, into an {@link ImageReader} of its own. The
 * native side copies the latest image and draws it with the rest of the frame, where frus
 * placed the widget: so a dialog opened over it covers it, and a page scrolling under it moves
 * it in the same frame. Touches reach it from the native side, which owns the input queue.
 *
 * <p>Every decision -- when to make, place or drop a view, which touches are its -- is made on
 * the native side (platform_view.rs); this file carries them out on the UI thread.
 *
 * Compiled into the bundled dex (scripts/build-input-dex.sh).
 */
public final class FrusPlatformViews {
    /** A new image of view {@code id} is ready to copy. */
    private static native void nativeFrame(int id);

    /** An event for the widget of view {@code id}. */
    private static native void nativeEvent(int id, String name, String data);

    /** An event for no view in particular (the web views' cookies, milestone 799). */
    private static native void nativeGlobal(String name, String data);

    /**
     * Which ways view {@code id}'s content can still scroll: 1 up, 2 down, 4 left, 8 right
     * (milestone 799). Sent when it changes, so that a drag on the view goes to the frus list
     * around it once the view is at its edge.
     */
    private static native void nativeScroll(int id, int flags);

    /** The event a view sends when it takes ({@code "true"}) or loses the input focus. */
    private static final String FOCUS = "frus.focus";
    /** The event that says a view could not be made, with the reason. */
    private static final String ERROR = "frus.error";

    private static Activity activity;
    private static final ClassLoader frusLoader = FrusPlatformViews.class.getClassLoader();
    private static FrameLayout container;
    private static Handler ui;
    private static final HashMap<String, PlatformViewFactory> factories =
            new HashMap<String, PlatformViewFactory>();
    private static final ConcurrentHashMap<Integer, Host> hosts =
            new ConcurrentHashMap<Integer, Host>();
    /** The view holding the input focus, or -1. */
    private static volatile int focused = -1;

    private FrusPlatformViews() {}

    /** Called once, the first time a widget asks for a native view. */
    public static synchronized void install(final Activity a) {
        if (activity != null) {
            return;
        }
        activity = a;
        ui = new Handler(Looper.getMainLooper());
        factories.put("frus/webview", new FrusWebViews());
        factories.put("frus/edittext", new EditTextFactory());
        ui.post(new Runnable() {
            @Override
            public void run() {
                container = new Container(a);
                a.addContentView(container, new ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
                container.getViewTreeObserver().addOnGlobalFocusChangeListener(
                        new ViewTreeObserver.OnGlobalFocusChangeListener() {
                            @Override
                            public void onGlobalFocusChanged(View before, View after) {
                                int was = focused;
                                int now = hostOf(after);
                                if (was == now) {
                                    return;
                                }
                                focused = now;
                                if (was >= 0) {
                                    nativeEvent(was, FOCUS, "false");
                                }
                                if (now >= 0) {
                                    nativeEvent(now, FOCUS, "true");
                                }
                            }
                        });
            }
        });
    }

    /**
     * A plugin's factory: {@code className} loaded from {@code dex} (or from the loader that
     * holds this class, when {@code dex} is null) and made with its no-argument constructor.
     * Returns null, or why it failed.
     */
    public static synchronized String register(String viewType, String className, ByteBuffer dex) {
        try {
            ClassLoader loader = dex == null
                    ? frusLoader
                    : new InMemoryDexClassLoader(dex, frusLoader);
            Class<?> type = loader.loadClass(className);
            Object made = type.getDeclaredConstructor().newInstance();
            factories.put(viewType, (PlatformViewFactory) made);
            return null;
        } catch (Throwable error) {
            return error.toString();
        }
    }

    private static synchronized PlatformViewFactory factory(String viewType) {
        return factories.get(viewType);
    }

    /** Makes view {@code id} of {@code viewType}, at {@code x, y} in the window, w by h pixels. */
    public static void create(final int id, final String viewType, final String params,
            final int x, final int y, final int w, final int h) {
        ui.post(new Runnable() {
            @Override
            public void run() {
                PlatformViewFactory factory = factory(viewType);
                if (factory == null) {
                    nativeEvent(id, ERROR, "no factory is registered for " + viewType);
                    return;
                }
                Host host = new Host(activity, id);
                PlatformView view;
                try {
                    view = factory.create(activity, id, params == null ? "" : params, host);
                } catch (Throwable error) {
                    nativeEvent(id, ERROR, error.toString());
                    return;
                }
                if (view == null || view.getView() == null) {
                    nativeEvent(id, ERROR, viewType + " made no view");
                    return;
                }
                host.view = view;
                host.addView(view.getView(), new FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
                hosts.put(id, host);
                container.addView(host, new FrameLayout.LayoutParams(w, h));
                host.place(x, y, w, h);
            }
        });
    }

    /** Moves or resizes view {@code id}. */
    public static void place(final int id, final int x, final int y, final int w, final int h) {
        ui.post(new Runnable() {
            @Override
            public void run() {
                Host host = hosts.get(id);
                if (host != null) {
                    host.place(x, y, w, h);
                }
            }
        });
    }

    /** The widget left: view {@code id} goes. */
    public static void dispose(final int id) {
        ui.post(new Runnable() {
            @Override
            public void run() {
                Host host = hosts.remove(id);
                if (host == null) {
                    return;
                }
                if (focused == id) {
                    focused = -1;
                    hideKeyboard();
                }
                try {
                    host.view.dispose();
                } catch (Throwable ignored) {
                    // The view is going whatever it did.
                }
                container.removeView(host);
                host.release();
            }
        });
    }

    /**
     * A touch for view {@code id}: {@code action} is MotionEvent's, {@code index} the pointer it
     * is about, {@code ids} and {@code xy} every pointer down, in the view's pixels.
     */
    public static void touch(final int id, final int action, final int index, final int[] ids,
            final float[] xy) {
        // Stamped when the native side hands it over, not when the UI thread gets round to it:
        // a fling's speed is read from these times.
        final long time = SystemClock.uptimeMillis();
        ui.post(new Runnable() {
            @Override
            public void run() {
                Host host = hosts.get(id);
                if (host != null) {
                    host.touch(action, index, ids, xy, time);
                }
            }
        });
    }

    /** A call from the application for view {@code id}. */
    public static void invoke(final int id, final String method, final String arguments) {
        ui.post(new Runnable() {
            @Override
            public void run() {
                Host host = hosts.get(id);
                if (host == null) {
                    return;
                }
                try {
                    host.view.onMethodCall(method, arguments == null ? "" : arguments);
                } catch (Throwable error) {
                    nativeEvent(id, ERROR, method + ": " + error);
                }
            }
        });
    }

    /** The activity was resumed ({@code true}) or paused. */
    public static void lifecycle(final boolean resumed) {
        ui.post(new Runnable() {
            @Override
            public void run() {
                for (Host host : hosts.values()) {
                    try {
                        if (resumed) {
                            host.view.onResume();
                        } else {
                            host.view.onPause();
                        }
                    } catch (Throwable ignored) {
                        // One view failing to pause must not keep the others running.
                    }
                }
            }
        });
    }

    /** A press went to frus: a native view holding the focus lets it go, and the keyboard. */
    public static void clearFocus() {
        if (focused < 0) {
            return;
        }
        ui.post(new Runnable() {
            @Override
            public void run() {
                if (container == null) {
                    return;
                }
                View view = container.findFocus();
                if (view != null) {
                    hideKeyboard();
                    view.clearFocus();
                }
            }
        });
    }

    /** A call for every web view rather than one (milestone 799): cookies, a page let go. */
    public static void webCall(final String method, final String arguments) {
        ui.post(new Runnable() {
            @Override
            public void run() {
                FrusWebViews.global(method, arguments == null ? "" : arguments);
            }
        });
    }

    /** An event for no view in particular. */
    static void sendGlobal(String name, String data) {
        nativeGlobal(name, data);
    }

    /** Whether a native view holds the input focus (the keyboard is then its, not frus's). */
    public static boolean hasFocus() {
        return focused >= 0;
    }

    /**
     * Copies the latest image of view {@code id} into {@code into} (width x height x 4 bytes,
     * RGBA, premultiplied, rows packed), writes its width and height into {@code size}, and
     * says whether there was one. An image larger than {@code into}, left from before a
     * resize, is dropped.
     */
    public static boolean copyFrame(int id, ByteBuffer into, int[] size) {
        Host host = hosts.get(id);
        return host != null && host.copy(into, size);
    }

    /** The view standing for widget view {@code id}, for the screen reader's tree. */
    static View hostFor(int id) {
        return hosts.get(id);
    }

    private static int hostOf(View view) {
        while (view != null) {
            if (view instanceof Host) {
                return ((Host) view).id;
            }
            ViewParent parent = view.getParent();
            view = parent instanceof View ? (View) parent : null;
        }
        return -1;
    }

    private static void hideKeyboard() {
        InputMethodManager imm =
                (InputMethodManager) activity.getSystemService(Context.INPUT_METHOD_SERVICE);
        if (imm != null && container != null) {
            imm.hideSoftInputFromWindow(container.getWindowToken(), 0);
        }
    }

    /**
     * Holds the hosts. Its children are left out of the window's own accessibility tree: each
     * one is shown to the screen reader inside the frus node that stands where it is drawn.
     */
    private static final class Container extends FrameLayout {
        Container(Context context) {
            super(context);
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
            setClipChildren(false);
        }

        @Override
        public void addChildrenForAccessibility(ArrayList<View> outChildren) {
            // On purpose: see the class comment.
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            super.onLayout(changed, left, top, right, bottom);
            // The container may have moved in the window (it is laid out after the first view
            // was placed, or the keyboard moved the content): the views keep their places.
            for (int i = 0; i < getChildCount(); i++) {
                View child = getChildAt(i);
                if (child instanceof Host) {
                    ((Host) child).translate();
                }
            }
        }
    }

    /** One embedded view: placed where frus laid it out, drawn into its own image. */
    private static final class Host extends FrameLayout
            implements Choreographer.FrameCallback, ImageReader.OnImageAvailableListener,
            PlatformViewChannel {
        final int id;
        PlatformView view;
        private ImageReader reader;
        private Surface surface;
        /** The image last produced and not copied yet; guarded by {@code this}. */
        private Image latest;
        private int width;
        private int height;
        /** Where frus placed it, in the window's pixels. */
        private int x;
        private int y;
        private boolean scheduled;
        private long downTime;
        /** The scroll directions last sent ({@link #nativeScroll}); -1 before the first. */
        private int scrollFlags = -1;

        Host(Context context, int id) {
            super(context);
            this.id = id;
            setWillNotDraw(false);
            setClipChildren(true);
            // Its own node, so that the view's nodes have it for their parent, and it the frus
            // node it is drawn in (FrusAccessibility.parentFor).
            setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        }

        @Override
        public void send(String name, String data) {
            nativeEvent(id, name == null ? "" : name, data == null ? "" : data);
        }

        /** At {@code x, y} in the window's pixels, {@code w} by {@code h}. */
        void place(int x, int y, int w, int h) {
            this.x = x;
            this.y = y;
            translate();
            if (w != width || h != height) {
                width = w;
                height = h;
                FrameLayout.LayoutParams params = (FrameLayout.LayoutParams) getLayoutParams();
                if (params != null) {
                    params.width = w;
                    params.height = h;
                    setLayoutParams(params);
                }
                ImageReader next = newReader(Math.max(1, w), Math.max(1, h));
                next.setOnImageAvailableListener(this, ui);
                ImageReader old;
                synchronized (this) {
                    old = reader;
                    reader = next;
                    surface = next.getSurface();
                    if (latest != null) {
                        latest.close();
                        latest = null;
                    }
                }
                if (old != null) {
                    old.close();
                }
                schedule();
            }
        }

        /** Puts the view where frus placed it, wherever the container is in the window. */
        void translate() {
            int[] origin = new int[2];
            container.getLocationInWindow(origin);
            setTranslationX(x - origin[0]);
            setTranslationY(y - origin[1]);
        }

        private static ImageReader newReader(int w, int h) {
            if (Build.VERSION.SDK_INT >= 29) {
                return ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 3,
                        HardwareBuffer.USAGE_GPU_COLOR_OUTPUT
                                | HardwareBuffer.USAGE_CPU_READ_OFTEN);
            }
            return ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 3);
        }

        void release() {
            Choreographer.getInstance().removeFrameCallback(this);
            scheduled = false;
            synchronized (this) {
                if (latest != null) {
                    latest.close();
                    latest = null;
                }
                if (reader != null) {
                    reader.close();
                    reader = null;
                    surface = null;
                }
            }
        }

        /** Draws the view on the next display frame, once however many times it is asked. */
        void schedule() {
            if (!scheduled) {
                scheduled = true;
                Choreographer.getInstance().postFrameCallback(this);
            }
        }

        @Override
        public void doFrame(long frameTimeNanos) {
            scheduled = false;
            Surface target;
            synchronized (this) {
                target = surface;
            }
            if (target == null || !target.isValid() || getWidth() == 0 || getHeight() == 0) {
                return;
            }
            Canvas canvas;
            try {
                canvas = Build.VERSION.SDK_INT >= 23
                        ? target.lockHardwareCanvas()
                        : target.lockCanvas(null);
            } catch (Throwable error) {
                return;
            }
            try {
                canvas.drawColor(Color.TRANSPARENT, PorterDuff.Mode.CLEAR);
                draw(canvas);
            } finally {
                target.unlockCanvasAndPost(canvas);
            }
            // Scrolling invalidates the view, so a drawn frame is when its edges may have moved.
            View content = view == null ? null : view.getView();
            if (content != null) {
                int flags = (content.canScrollVertically(-1) ? 1 : 0)
                        | (content.canScrollVertically(1) ? 2 : 0)
                        | (content.canScrollHorizontally(-1) ? 4 : 0)
                        | (content.canScrollHorizontally(1) ? 8 : 0);
                if (flags != scrollFlags) {
                    scrollFlags = flags;
                    nativeScroll(id, flags);
                }
            }
        }

        @Override
        public void onImageAvailable(ImageReader from) {
            Image image;
            try {
                image = from.acquireLatestImage();
            } catch (Throwable error) {
                return;
            }
            if (image == null) {
                return;
            }
            synchronized (this) {
                if (from != reader) {
                    image.close();
                    return;
                }
                // Only the newest image is kept: the queue never fills, so drawing never waits
                // for the native side, however long it takes to come for it.
                if (latest != null) {
                    latest.close();
                }
                latest = image;
            }
            nativeFrame(id);
        }

        synchronized boolean copy(ByteBuffer into, int[] size) {
            Image image = latest;
            if (image == null) {
                return false;
            }
            latest = null;
            try {
                int w = image.getWidth();
                int h = image.getHeight();
                int row = w * 4;
                if ((long) row * h > into.capacity()) {
                    return false;
                }
                Image.Plane plane = image.getPlanes()[0];
                ByteBuffer pixels = plane.getBuffer();
                int stride = plane.getRowStride();
                into.clear();
                if (stride == row) {
                    pixels.limit(Math.min(pixels.capacity(), row * h));
                    pixels.position(0);
                    into.put(pixels);
                } else {
                    for (int y = 0; y < h; y++) {
                        int start = y * stride;
                        pixels.limit(start + row);
                        pixels.position(start);
                        into.put(pixels);
                    }
                }
                size[0] = w;
                size[1] = h;
                return true;
            } catch (Throwable error) {
                return false;
            } finally {
                image.close();
            }
        }

        void touch(int action, int index, int[] ids, float[] xy, long time) {
            int count = ids.length;
            if (count == 0) {
                return;
            }
            if (action == MotionEvent.ACTION_DOWN) {
                downTime = time;
            }
            MotionEvent.PointerProperties[] properties = new MotionEvent.PointerProperties[count];
            MotionEvent.PointerCoords[] coords = new MotionEvent.PointerCoords[count];
            for (int i = 0; i < count; i++) {
                properties[i] = new MotionEvent.PointerProperties();
                properties[i].id = ids[i];
                properties[i].toolType = MotionEvent.TOOL_TYPE_FINGER;
                coords[i] = new MotionEvent.PointerCoords();
                coords[i].x = xy[2 * i];
                coords[i].y = xy[2 * i + 1];
                coords[i].pressure = 1f;
                coords[i].size = 1f;
            }
            int masked = action;
            if (action == MotionEvent.ACTION_POINTER_DOWN || action == MotionEvent.ACTION_POINTER_UP) {
                masked = action | (index << MotionEvent.ACTION_POINTER_INDEX_SHIFT);
            }
            MotionEvent event = MotionEvent.obtain(downTime, time, masked, count, properties,
                    coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0);
            dispatchTouchEvent(event);
            event.recycle();
        }

        @Override
        protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
            super.onLayout(changed, left, top, right, bottom);
            schedule();
        }

        // The view tree is never drawn by the window: a change anywhere inside is redrawn here.

        @Override
        public void onDescendantInvalidated(View child, View target) {
            super.onDescendantInvalidated(child, target);
            schedule();
        }

        @Override
        public ViewParent invalidateChildInParent(int[] location, Rect dirty) {
            schedule();
            return super.invalidateChildInParent(location, dirty);
        }

        @Override
        public void onInitializeAccessibilityNodeInfo(AccessibilityNodeInfo info) {
            super.onInitializeAccessibilityNodeInfo(info);
            FrusAccessibility.parentFor(id, info);
        }
    }

    // --- Two views every application can embed with no plugin --------------------------

    /**
     * {@code frus/edittext}: an EditText. Its parameters are its hint. Calls: {@code setText},
     * {@code setHint}. Events: {@code changed} (the text, on every change) and
     * {@code submitted} (the text, on the keyboard's action key).
     */
    static final class EditTextFactory implements PlatformViewFactory {
        @Override
        public PlatformView create(Context context, int id, String params,
                final PlatformViewChannel channel) {
            final EditText edit = new EditText(context);
            edit.setHint(params);
            edit.setSingleLine(true);
            edit.setImeOptions(EditorInfo.IME_ACTION_DONE);
            // Its own colours: the activity's theme may be dark, which would write white on it.
            edit.setBackgroundColor(Color.WHITE);
            edit.setTextColor(Color.BLACK);
            edit.setHintTextColor(Color.GRAY);
            edit.addTextChangedListener(new TextWatcher() {
                @Override
                public void beforeTextChanged(CharSequence s, int start, int count, int after) {}

                @Override
                public void onTextChanged(CharSequence s, int start, int before, int count) {}

                @Override
                public void afterTextChanged(Editable s) {
                    channel.send("changed", s.toString());
                }
            });
            edit.setOnEditorActionListener(new TextView.OnEditorActionListener() {
                @Override
                public boolean onEditorAction(TextView view, int action, KeyEvent event) {
                    channel.send("submitted", view.getText().toString());
                    return false;
                }
            });
            return new PlatformView() {
                @Override
                public View getView() {
                    return edit;
                }

                @Override
                public void dispose() {}

                @Override
                public void onMethodCall(String method, String arguments) {
                    if ("setText".equals(method)) {
                        edit.setText(arguments);
                    } else if ("setHint".equals(method)) {
                        edit.setHint(arguments);
                    }
                }
            };
        }
    }
}
