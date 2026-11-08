package dev.frus.input;

import android.app.Activity;
import android.content.Context;
import android.graphics.Rect;
import android.os.Build;
import android.os.Bundle;
import android.util.SparseIntArray;
import android.view.View;
import android.view.ViewGroup;
import android.view.ViewParent;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.accessibility.AccessibilityNodeInfo;
import android.view.accessibility.AccessibilityNodeProvider;

/**
 * The screen reader's view of a frus window (milestone 782).
 *
 * <p>A frus window is drawn, not made of views, so this one view, as large as the
 * content, stands for all of it: its node provider shows TalkBack one virtual view per
 * node of the frame. Every row of the tree -- its parent, its box, its class, its words,
 * its states and actions -- was worked out on the native side and is applied here
 * unchanged; this file is a pipe, not a policy. The one thing kept here is TalkBack's
 * own focus (the green box), which belongs to the screen reader and never reaches the
 * application.
 *
 * <p>The tree is published whole from the native thread and swapped in on the UI
 * thread, where the screen reader reads it, together with the events that say what
 * changed. Requests go back through {@code nativePerform}, checked first against the
 * published tree so the answer TalkBack gets is true.
 *
 * Compiled into the bundled dex (scripts/build-input-dex.sh).
 */
public final class FrusAccessibility extends View {
    /** Whether any accessibility service is on: the native side builds the tree only then. */
    private static native void nativeEnabled(boolean on);

    /** A request for virtual view {@code id}: an action, with set text's text or set progress's value. */
    private static native void nativePerform(int id, int action, String text, float value);

    // The row flags, the native side's (a11y_android.rs, `flag`).
    private static final int CHECKABLE = 1;
    private static final int CHECKED = 1 << 1;
    private static final int ENABLED = 1 << 2;
    private static final int FOCUSABLE = 1 << 3;
    private static final int FOCUSED = 1 << 4;
    private static final int CLICKABLE = 1 << 5;
    private static final int EDITABLE = 1 << 6;
    private static final int SCROLLABLE = 1 << 7;
    private static final int HEADING = 1 << 8;
    private static final int PASSWORD = 1 << 9;
    private static final int SELECTED = 1 << 10;
    private static final int VISIBLE = 1 << 11;
    private static final int RANGE = 1 << 12;
    private static final int SET_PROGRESS = 1 << 13;

    /** {@code android.R.id.accessibilityActionSetProgress}. */
    private static final int ACTION_SET_PROGRESS = 0x0102003d;
    private static final int NO_FOCUS = Integer.MIN_VALUE;
    /** How long content changes are gathered into one event, as views do. */
    private static final long CONTENT_CHANGE_DELAY_MS = 100;

    private static volatile FrusAccessibility instance;

    /** One published tree: parallel arrays, one entry a row, read only once built. */
    private static final class Tree {
        final int[] ids;
        final int[] parents;
        final int[] bounds;
        final int[] flags;
        final int[] actions;
        final String[] classes;
        final String[] texts;
        final String[] descriptions;
        final String[] hints;
        final float[] ranges;
        final int[] scrolls;
        final SparseIntArray index = new SparseIntArray();

        Tree(int[] ids, int[] parents, int[] bounds, int[] flags, int[] actions,
                String[] classes, String[] texts, String[] descriptions, String[] hints,
                float[] ranges, int[] scrolls) {
            this.ids = ids;
            this.parents = parents;
            this.bounds = bounds;
            this.flags = flags;
            this.actions = actions;
            this.classes = classes;
            this.texts = texts;
            this.descriptions = descriptions;
            this.hints = hints;
            this.ranges = ranges;
            this.scrolls = scrolls;
            for (int i = 0; i < ids.length; i++) {
                index.put(ids[i], i);
            }
        }

        int find(int id) {
            return index.get(id, -1);
        }
    }

    private static final Tree EMPTY = new Tree(new int[0], new int[0], new int[0], new int[0],
            new int[0], new String[0], new String[0], new String[0], new String[0],
            new float[0], new int[0]);

    /** The tree the screen reader reads; replaced only on the UI thread. */
    private Tree tree = EMPTY;
    /** The node TalkBack's focus is on, or {@link #NO_FOCUS}. */
    private int accessibilityFocus = NO_FOCUS;
    private boolean contentChangePending;
    private final AccessibilityManager manager;
    private final Provider provider = new Provider();
    private final Runnable sendContentChanged = new Runnable() {
        @Override
        public void run() {
            contentChangePending = false;
            AccessibilityEvent event = obtain(AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED);
            if (event == null) {
                return;
            }
            event.setContentChangeTypes(AccessibilityEvent.CONTENT_CHANGE_TYPE_SUBTREE);
            event.setSource(FrusAccessibility.this);
            send(event);
        }
    };

    /** Adds the view over the content, once, and says whether a service is on. */
    public static void install(final Activity activity) {
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (instance != null) {
                    return;
                }
                FrusAccessibility view = new FrusAccessibility(activity);
                activity.addContentView(view, new ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT));
                instance = view;
                nativeEnabled(view.manager != null && view.manager.isEnabled());
            }
        });
    }

    /** A new tree, the events that say what changed, and words to read out ({@code null} for none). */
    public static void publish(int[] ids, int[] parents, int[] bounds, int[] flags,
            int[] actions, String[] classes, String[] texts, String[] descriptions,
            String[] hints, float[] ranges, int[] scrolls, final int[] events,
            final String[] eventTexts, final String announce) {
        final FrusAccessibility view = instance;
        if (view == null) {
            return;
        }
        final Tree next = new Tree(ids, parents, bounds, flags, actions, classes, texts,
                descriptions, hints, ranges, scrolls);
        view.post(new Runnable() {
            @Override
            public void run() {
                view.apply(next, events, eventTexts, announce);
            }
        });
    }

    private FrusAccessibility(Context context) {
        super(context);
        manager = (AccessibilityManager) context.getSystemService(Context.ACCESSIBILITY_SERVICE);
        setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_YES);
        // It stands for the drawing; it never takes a touch, a key or the focus itself.
        setFocusable(false);
        setClickable(false);
        if (Build.VERSION.SDK_INT >= 26) {
            setImportantForAutofill(IMPORTANT_FOR_AUTOFILL_NO);
        }
        if (manager != null) {
            manager.addAccessibilityStateChangeListener(
                    new AccessibilityManager.AccessibilityStateChangeListener() {
                        @Override
                        public void onAccessibilityStateChanged(boolean enabled) {
                            if (!enabled) {
                                accessibilityFocus = NO_FOCUS;
                            }
                            nativeEnabled(enabled);
                        }
                    });
        }
    }

    @Override
    public AccessibilityNodeProvider getAccessibilityNodeProvider() {
        return provider;
    }

    private void apply(Tree next, int[] events, String[] eventTexts, String announce) {
        tree = next;
        if (accessibilityFocus != NO_FOCUS && next.find(accessibilityFocus) < 0) {
            // The node TalkBack was on has gone: so has its focus.
            accessibilityFocus = NO_FOCUS;
        }
        for (int i = 0; i + 5 < events.length; i += 6) {
            raise(next, events[i], events[i + 1], events[i + 2], events[i + 3], events[i + 4],
                    events[i + 5], eventTexts[i / 6]);
        }
        if (announce != null && announce.length() > 0 && isOn()) {
            announceForAccessibility(announce);
        }
    }

    private boolean isOn() {
        return manager != null && manager.isEnabled();
    }

    private AccessibilityEvent obtain(int type) {
        if (!isOn()) {
            return null;
        }
        AccessibilityEvent event = AccessibilityEvent.obtain(type);
        event.setPackageName(getContext().getPackageName());
        return event;
    }

    private void send(AccessibilityEvent event) {
        ViewParent parent = getParent();
        if (parent != null) {
            parent.requestSendAccessibilityEvent(this, event);
        }
    }

    /** An event from virtual view {@code id}, filled the way a view fills its own. */
    private AccessibilityEvent eventFor(Tree t, int type, int id) {
        int i = t.find(id);
        if (i < 0) {
            return null;
        }
        AccessibilityEvent event = obtain(type);
        if (event == null) {
            return null;
        }
        event.setSource(this, id);
        event.setClassName(t.classes[i]);
        int f = t.flags[i];
        event.setEnabled((f & ENABLED) != 0);
        event.setChecked((f & CHECKED) != 0);
        event.setPassword((f & PASSWORD) != 0);
        if (t.descriptions[i] != null) {
            event.setContentDescription(t.descriptions[i]);
        }
        if (t.texts[i] != null) {
            event.getText().add(t.texts[i]);
        } else if (t.descriptions[i] != null) {
            event.getText().add(t.descriptions[i]);
        }
        if ((f & RANGE) != 0) {
            float min = t.ranges[3 * i];
            float max = t.ranges[3 * i + 1];
            float current = t.ranges[3 * i + 2];
            // As a seek bar reports itself: out of a hundred.
            event.setItemCount(100);
            event.setCurrentItemIndex(max > min ? Math.round(100 * (current - min) / (max - min)) : 0);
        }
        return event;
    }

    private void raise(Tree t, int type, int id, int a, int b, int c, int d, String before) {
        if (type == AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED) {
            if (!contentChangePending && isOn()) {
                contentChangePending = true;
                postDelayed(sendContentChanged, CONTENT_CHANGE_DELAY_MS);
            }
            return;
        }
        AccessibilityEvent event = eventFor(t, type, id);
        if (event == null) {
            return;
        }
        if (type == AccessibilityEvent.TYPE_VIEW_TEXT_CHANGED) {
            event.setFromIndex(a);
            event.setRemovedCount(b);
            event.setAddedCount(c);
            event.setBeforeText(before != null ? before : "");
        } else if (type == AccessibilityEvent.TYPE_VIEW_SCROLLED) {
            event.setScrollX(a);
            event.setScrollY(b);
            event.setMaxScrollX(c);
            event.setMaxScrollY(d);
        }
        send(event);
    }

    private void sendSimple(int type, int id) {
        AccessibilityEvent event = eventFor(tree, type, id);
        if (event != null) {
            send(event);
        }
    }

    /** The node provider: the published rows, as {@link AccessibilityNodeInfo}s. */
    private final class Provider extends AccessibilityNodeProvider {
        @Override
        public AccessibilityNodeInfo createAccessibilityNodeInfo(int id) {
            Tree t = tree;
            FrusAccessibility host = FrusAccessibility.this;
            if (id == HOST_VIEW_ID) {
                AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(host);
                onInitializeAccessibilityNodeInfo(info);
                for (int i = 0; i < t.ids.length; i++) {
                    if (t.parents[i] == HOST_VIEW_ID) {
                        info.addChild(host, t.ids[i]);
                    }
                }
                return info;
            }
            int i = t.find(id);
            if (i < 0) {
                return null;
            }
            AccessibilityNodeInfo info = AccessibilityNodeInfo.obtain(host, id);
            info.setPackageName(getContext().getPackageName());
            info.setClassName(t.classes[i]);
            if (t.parents[i] == HOST_VIEW_ID) {
                info.setParent(host);
            } else {
                info.setParent(host, t.parents[i]);
            }
            for (int j = i + 1; j < t.ids.length; j++) {
                if (t.parents[j] == id) {
                    info.addChild(host, t.ids[j]);
                }
            }
            // The boxes are the window's pixels: the drawing fills the window, while this view
            // sits in the content frame, maybe below the status bar.
            int[] window = new int[2];
            int[] self = new int[2];
            getRootView().getLocationOnScreen(window);
            getLocationOnScreen(self);
            int left = t.bounds[4 * i];
            int top = t.bounds[4 * i + 1];
            int right = t.bounds[4 * i + 2];
            int bottom = t.bounds[4 * i + 3];
            info.setBoundsInScreen(new Rect(left + window[0], top + window[1],
                    right + window[0], bottom + window[1]));
            int dx = self[0] - window[0];
            int dy = self[1] - window[1];
            info.setBoundsInParent(new Rect(left - dx, top - dy, right - dx, bottom - dy));

            int f = t.flags[i];
            info.setVisibleToUser((f & VISIBLE) != 0);
            info.setEnabled((f & ENABLED) != 0);
            info.setCheckable((f & CHECKABLE) != 0);
            info.setChecked((f & CHECKED) != 0);
            info.setFocusable((f & FOCUSABLE) != 0);
            info.setFocused((f & FOCUSED) != 0);
            info.setClickable((f & CLICKABLE) != 0);
            info.setEditable((f & EDITABLE) != 0);
            info.setScrollable((f & SCROLLABLE) != 0);
            info.setPassword((f & PASSWORD) != 0);
            info.setSelected((f & SELECTED) != 0);
            if (Build.VERSION.SDK_INT >= 28) {
                info.setHeading((f & HEADING) != 0);
            }
            if (t.texts[i] != null) {
                info.setText(t.texts[i]);
            }
            if (t.descriptions[i] != null) {
                info.setContentDescription(t.descriptions[i]);
            }
            if (t.hints[i] != null && Build.VERSION.SDK_INT >= 26) {
                info.setHintText(t.hints[i]);
            } else if (t.hints[i] != null && t.texts[i] == null && t.descriptions[i] == null) {
                info.setContentDescription(t.hints[i]);
            }
            if ((f & RANGE) != 0) {
                info.setRangeInfo(AccessibilityNodeInfo.RangeInfo.obtain(
                        AccessibilityNodeInfo.RangeInfo.RANGE_TYPE_FLOAT,
                        t.ranges[3 * i], t.ranges[3 * i + 1], t.ranges[3 * i + 2]));
            }
            if ((f & SET_PROGRESS) != 0 && Build.VERSION.SDK_INT >= 24) {
                info.addAction(AccessibilityNodeInfo.AccessibilityAction.ACTION_SET_PROGRESS);
            }
            if (t.actions[i] != 0) {
                info.addAction(t.actions[i]);
            }
            boolean focused = accessibilityFocus == id;
            info.setAccessibilityFocused(focused);
            info.addAction(focused
                    ? AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS
                    : AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS);
            return info;
        }

        @Override
        public AccessibilityNodeInfo findFocus(int focus) {
            Tree t = tree;
            if (focus == AccessibilityNodeInfo.FOCUS_ACCESSIBILITY) {
                return accessibilityFocus == NO_FOCUS ? null
                        : createAccessibilityNodeInfo(accessibilityFocus);
            }
            if (focus == AccessibilityNodeInfo.FOCUS_INPUT) {
                for (int i = 0; i < t.ids.length; i++) {
                    if ((t.flags[i] & FOCUSED) != 0) {
                        return createAccessibilityNodeInfo(t.ids[i]);
                    }
                }
            }
            return null;
        }

        @Override
        public boolean performAction(int id, int action, Bundle arguments) {
            if (id == HOST_VIEW_ID) {
                return performAccessibilityAction(action, arguments);
            }
            Tree t = tree;
            int i = t.find(id);
            if (i < 0) {
                return false;
            }
            switch (action) {
                case AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS:
                    if (accessibilityFocus != id) {
                        int before = accessibilityFocus;
                        accessibilityFocus = id;
                        if (before != NO_FOCUS) {
                            sendSimple(AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED, before);
                        }
                        invalidate();
                        sendSimple(AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUSED, id);
                    }
                    return true;
                case AccessibilityNodeInfo.ACTION_CLEAR_ACCESSIBILITY_FOCUS:
                    if (accessibilityFocus == id) {
                        accessibilityFocus = NO_FOCUS;
                        invalidate();
                        sendSimple(AccessibilityEvent.TYPE_VIEW_ACCESSIBILITY_FOCUS_CLEARED, id);
                    }
                    return true;
                case ACTION_SET_PROGRESS:
                    if ((t.flags[i] & SET_PROGRESS) == 0 || arguments == null
                            || !arguments.containsKey(
                                    AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE)) {
                        return false;
                    }
                    nativePerform(id, action, null, arguments.getFloat(
                            AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE));
                    return true;
                default:
                    if ((t.actions[i] & action) == 0 || Integer.bitCount(action) != 1) {
                        return false;
                    }
                    String text = null;
                    if (action == AccessibilityNodeInfo.ACTION_SET_TEXT) {
                        CharSequence value = arguments == null ? null : arguments.getCharSequence(
                                AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE);
                        text = value == null ? "" : value.toString();
                    }
                    nativePerform(id, action, text, 0f);
                    if (action == AccessibilityNodeInfo.ACTION_CLICK
                            && (t.flags[i] & CHECKABLE) == 0) {
                        // A box says its new state once it has one (the native side raises
                        // the click then); anything else is clicked now.
                        sendSimple(AccessibilityEvent.TYPE_VIEW_CLICKED, id);
                    }
                    return true;
            }
        }
    }
}
