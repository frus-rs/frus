package dev.frus.input;

import android.app.Activity;
import android.app.Application;
import android.app.Fragment;
import android.app.FragmentManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ActivityInfo;
import android.content.pm.PackageManager;
import android.content.pm.ResolveInfo;
import android.os.Build;
import android.os.Bundle;
import android.view.textservice.SentenceSuggestionsInfo;
import android.view.textservice.SpellCheckerSession;
import android.view.textservice.SuggestionsInfo;
import android.view.textservice.TextInfo;
import android.view.textservice.TextServicesManager;
import java.util.ArrayList;
import java.util.List;

/**
 * The system's text services for the native text fields (milestone 829): the spell checker,
 * through a {@link SpellCheckerSession}, and the "process text" actions other applications
 * offer on a selection ({@code Intent.ACTION_PROCESS_TEXT}).
 *
 * <p>Like the rest of the bridge, a pipe: the native side decides what to check and when,
 * and what to do with the answers. Nothing here runs until the native side first asks.
 */
public final class FrusTextServices {
    private FrusTextServices() {}

    /**
     * The spell checker's answer to request {@code sequence}: for each misspelled word, its
     * start and length in UTF-16 units of the text sent, and its suggestions joined by
     * newlines. {@code starts} is null when no spell checker is available.
     */
    private static native void nativeSpellResults(int sequence, int[] starts, int[] lengths,
            String[] suggestions);

    /** The text an action returned for the selection it was handed, or null for none. */
    private static native void nativeProcessTextResult(String text);

    /** How many suggestions to ask the checker for, per word. */
    private static final int MAX_SUGGESTIONS = 5;

    private static SpellCheckerSession session;
    private static boolean sessionTried;
    /** The request the checker is answering, so its answer can be told apart from a later one. */
    private static int pendingSequence;

    /**
     * Has the system's spell checker read {@code text}, answering through
     * {@code nativeSpellResults} with {@code sequence}. The session is opened on the UI thread
     * with the first request, and kept.
     */
    public static void spellCheck(final Activity activity, final int sequence, final String text) {
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                SpellCheckerSession checker = session(activity);
                if (checker == null) {
                    nativeSpellResults(sequence, null, null, null);
                    return;
                }
                pendingSequence = sequence;
                checker.getSentenceSuggestions(
                        new TextInfo[] {new TextInfo(text)}, MAX_SUGGESTIONS);
            }
        });
    }

    private static SpellCheckerSession session(Activity activity) {
        if (sessionTried) {
            return session;
        }
        sessionTried = true;
        TextServicesManager manager = (TextServicesManager)
                activity.getSystemService(Context.TEXT_SERVICES_MANAGER_SERVICE);
        if (manager == null) {
            return null;
        }
        // The language the person chose for the spell checker, as a platform field uses.
        session = manager.newSpellCheckerSession(null, null, new Listener(), true);
        return session;
    }

    private static final class Listener implements SpellCheckerSession.SpellCheckerSessionListener {
        @Override
        public void onGetSuggestions(SuggestionsInfo[] results) {}

        @Override
        public void onGetSentenceSuggestions(SentenceSuggestionsInfo[] results) {
            ArrayList<int[]> words = new ArrayList<int[]>();
            ArrayList<String> offered = new ArrayList<String>();
            if (results != null) {
                for (SentenceSuggestionsInfo sentence : results) {
                    if (sentence == null) {
                        continue;
                    }
                    for (int i = 0; i < sentence.getSuggestionsCount(); i++) {
                        SuggestionsInfo info = sentence.getSuggestionsInfoAt(i);
                        if (info == null) {
                            continue;
                        }
                        int attributes = info.getSuggestionsAttributes();
                        boolean typo = (attributes & SuggestionsInfo.RESULT_ATTR_LOOKS_LIKE_TYPO) != 0;
                        boolean known = (attributes & SuggestionsInfo.RESULT_ATTR_IN_THE_DICTIONARY) != 0;
                        if (!typo || known) {
                            continue;
                        }
                        StringBuilder joined = new StringBuilder();
                        for (int s = 0; s < Math.max(0, info.getSuggestionsCount()); s++) {
                            if (s > 0) {
                                joined.append('\n');
                            }
                            joined.append(info.getSuggestionAt(s));
                        }
                        words.add(new int[] {sentence.getOffsetAt(i), sentence.getLengthAt(i)});
                        offered.add(joined.toString());
                    }
                }
            }
            int[] starts = new int[words.size()];
            int[] lengths = new int[words.size()];
            for (int i = 0; i < words.size(); i++) {
                starts[i] = words.get(i)[0];
                lengths[i] = words.get(i)[1];
            }
            nativeSpellResults(pendingSequence, starts, lengths,
                    offered.toArray(new String[0]));
        }
    }

    // --- Process text ------------------------------------------------------------------

    /** The activities found by the last {@link #processTextLabels}, in the same order. */
    private static List<ResolveInfo> processors = new ArrayList<ResolveInfo>();

    private static Intent processTextIntent() {
        return new Intent().setAction(Intent.ACTION_PROCESS_TEXT).setType("text/plain");
    }

    /**
     * The words of every installed activity that acts on selected text, in the platform's
     * order; an action is known to the native side by its index here.
     */
    public static String[] processTextLabels(Activity activity) {
        PackageManager manager = activity.getPackageManager();
        List<ResolveInfo> found = manager.queryIntentActivities(processTextIntent(), 0);
        ArrayList<ResolveInfo> kept = new ArrayList<ResolveInfo>();
        ArrayList<String> labels = new ArrayList<String>();
        for (ResolveInfo info : found) {
            ActivityInfo activityInfo = info.activityInfo;
            // The application's own activities are not offered to itself, and one another
            // application keeps for itself cannot be started.
            if (activityInfo == null || !activityInfo.exported
                    || activityInfo.packageName.equals(activity.getPackageName())) {
                continue;
            }
            CharSequence label = info.loadLabel(manager);
            kept.add(info);
            labels.add(label == null ? activityInfo.name : label.toString());
        }
        processors = kept;
        return labels.toArray(new String[0]);
    }

    /**
     * Hands {@code text} to action {@code index}. A read-only field's text goes as read-only,
     * and the action answers nothing; otherwise what it returns comes back through
     * {@code nativeProcessTextResult}.
     */
    public static void processText(final Activity activity, final int index, final String text,
            final boolean readOnly) {
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                if (index < 0 || index >= processors.size()) {
                    return;
                }
                ActivityInfo target = processors.get(index).activityInfo;
                Intent intent = processTextIntent()
                        .setComponent(new ComponentName(target.packageName, target.name))
                        .putExtra(Intent.EXTRA_PROCESS_TEXT, text)
                        .putExtra(Intent.EXTRA_PROCESS_TEXT_READONLY, readOnly);
                try {
                    if (readOnly) {
                        activity.startActivity(intent);
                    } else {
                        ResultFragment.start(activity, intent);
                    }
                } catch (RuntimeException e) {
                    // An application uninstalled since the menu was built, or one that refuses.
                    android.util.Log.w("frus", "process text: " + e);
                }
            }
        });
    }

    /**
     * Where the answer of an action comes back. A NativeActivity cannot override
     * {@code onActivityResult}, but a fragment it holds can: one with no view is added for
     * the request, and taken away again once it is answered.
     */
    public static final class ResultFragment extends Fragment {
        private static final String TAG = "dev.frus.process-text";
        private static final int REQUEST = 0x7f29;
        private static boolean guarded;

        /** Required of a fragment; never used to bring one back (see {@link #guard}). */
        public ResultFragment() {}

        static void start(Activity activity, Intent intent) {
            guard(activity);
            FragmentManager manager = activity.getFragmentManager();
            Fragment old = manager.findFragmentByTag(TAG);
            ResultFragment fragment = new ResultFragment();
            android.app.FragmentTransaction transaction = manager.beginTransaction();
            if (old != null) {
                transaction.remove(old);
            }
            transaction.add(fragment, TAG);
            if (Build.VERSION.SDK_INT >= 24) {
                transaction.commitNow();
            } else {
                transaction.commit();
                manager.executePendingTransactions();
            }
            fragment.startActivityForResult(intent, REQUEST);
        }

        @Override
        public void onActivityResult(int requestCode, int resultCode, Intent data) {
            if (requestCode == REQUEST) {
                String text = null;
                if (resultCode == Activity.RESULT_OK && data != null) {
                    CharSequence answer = data.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT);
                    if (answer != null) {
                        text = answer.toString();
                    }
                }
                nativeProcessTextResult(text);
                Activity activity = getActivity();
                if (activity != null && !activity.isFinishing()) {
                    getFragmentManager().beginTransaction().remove(this).commitAllowingStateLoss();
                }
            }
        }

        /**
         * This class lives in a dex the application's own class loader cannot see. A fragment
         * the activity saved would be brought back with that loader, and fail, taking the
         * activity down with it; so the saved fragments are taken out of the activity's
         * state as it is saved. frus adds no other fragment, so nothing else is lost; an
         * answer that arrives after the activity was made again is dropped.
         */
        private static void guard(Activity activity) {
            if (guarded) {
                return;
            }
            guarded = true;
            activity.getApplication().registerActivityLifecycleCallbacks(
                    new Application.ActivityLifecycleCallbacks() {
                        @Override
                        public void onActivitySaveInstanceState(Activity a, Bundle state) {
                            state.remove("android:fragments");
                        }

                        @Override
                        public void onActivityCreated(Activity a, Bundle state) {}

                        @Override
                        public void onActivityStarted(Activity a) {}

                        @Override
                        public void onActivityResumed(Activity a) {}

                        @Override
                        public void onActivityPaused(Activity a) {}

                        @Override
                        public void onActivityStopped(Activity a) {}

                        @Override
                        public void onActivityDestroyed(Activity a) {}
                    });
        }
    }
}
