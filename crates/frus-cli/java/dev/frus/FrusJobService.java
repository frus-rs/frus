// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

package dev.frus;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.job.JobInfo;
import android.app.job.JobParameters;
import android.app.job.JobScheduler;
import android.app.job.JobService;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.Bundle;
import android.os.PersistableBundle;
import android.util.Log;

/**
 * Runs a frus application's headless entry points with no window (milestone 828).
 *
 * <p>The system starts this service when a job the application scheduled is due, in the
 * application's process: started for the job when nothing of the application runs, or the
 * running one. The service loads the application's native library (named by the manifest's
 * {@code dev.frus.lib_name} meta-data, which {@code frus build} writes) and calls the entry
 * point the job names, on a thread of its own: {@code onStartJob} runs on the main thread and
 * must return at once.
 *
 * <p>{@code frus build} and {@code frus run} compile this class into the package and declare
 * it in the manifest only for an application whose library declares headless entry points.
 */
public class FrusJobService extends JobService {
    private static final String TAG = "frus";
    private static final String ENTRY = "frus.entry";
    private static final String INPUT = "frus.input";
    private static final String CHANNEL = "frus.background";

    /** The outcomes {@link #nativeRun} answers. */
    private static final int DONE = 0;
    private static final int RETRY = 1;

    /** Runs the entry point {@code name}: 0 done, 1 to retry later, 2 no such entry. */
    private static native int nativeRun(
            Context context, String name, String input, String files, int job);

    /** Asks the entry point running for {@code job} to stop. */
    private static native void nativeStop(int job);

    @Override
    public boolean onStartJob(final JobParameters params) {
        final PersistableBundle extras = params.getExtras();
        final String name = extras.getString(ENTRY);
        final String input = extras.getString(INPUT);
        if (name == null) {
            return false;
        }
        final Context context = getApplicationContext();
        new Thread(new Runnable() {
            @Override
            public void run() {
                int outcome = RETRY;
                try {
                    System.loadLibrary(library(context));
                    outcome = nativeRun(
                            context,
                            name,
                            input,
                            context.getFilesDir().getAbsolutePath(),
                            params.getJobId());
                } catch (Throwable failure) {
                    Log.e(TAG, "background job " + name + " could not run", failure);
                    outcome = DONE;
                }
                jobFinished(params, outcome == RETRY);
            }
        }, "frus-job-" + name).start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        try {
            nativeStop(params.getJobId());
        } catch (Throwable notLoaded) {
            // The library never loaded: nothing runs.
        }
        return true;
    }

    /** The application's native library, named by the service's meta-data. */
    private static String library(Context context) throws PackageManager.NameNotFoundException {
        ServiceInfo info = context.getPackageManager().getServiceInfo(
                new ComponentName(context, FrusJobService.class), PackageManager.GET_META_DATA);
        Bundle meta = info.metaData;
        String name = meta == null ? null : meta.getString("dev.frus.lib_name");
        if (name == null) {
            throw new IllegalStateException("the manifest names no dev.frus.lib_name");
        }
        return name;
    }

    /** The job id of the entry point {@code name}: the same for every schedule of it. */
    static int jobId(String name) {
        return ("frus.job:" + name).hashCode();
    }

    /**
     * Schedules the entry point {@code name}. {@code periodMs} 0 runs it once, after
     * {@code delayMs}; otherwise every {@code periodMs}. {@code network}: 0 none needed, 1 any,
     * 2 unmetered. Answers 1 when the system took it, 0 when it refused.
     */
    public static int schedule(Context context, String name, String input, long periodMs,
            long delayMs, int network, boolean charging, boolean idle, boolean batteryNotLow,
            boolean persisted) {
        JobScheduler scheduler =
                (JobScheduler) context.getSystemService(Context.JOB_SCHEDULER_SERVICE);
        if (scheduler == null) {
            return 0;
        }
        PersistableBundle extras = new PersistableBundle();
        extras.putString(ENTRY, name);
        if (input != null) {
            extras.putString(INPUT, input);
        }
        JobInfo.Builder job = new JobInfo.Builder(
                jobId(name), new ComponentName(context, FrusJobService.class));
        job.setExtras(extras);
        if (periodMs > 0) {
            job.setPeriodic(periodMs);
        } else if (delayMs > 0) {
            job.setMinimumLatency(delayMs);
        }
        if (network == 1) {
            job.setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY);
        } else if (network == 2) {
            job.setRequiredNetworkType(JobInfo.NETWORK_TYPE_UNMETERED);
        }
        job.setRequiresCharging(charging);
        job.setRequiresDeviceIdle(idle);
        if (Build.VERSION.SDK_INT >= 26) {
            job.setRequiresBatteryNotLow(batteryNotLow);
        }
        job.setPersisted(persisted);
        return scheduler.schedule(job.build()) == JobScheduler.RESULT_SUCCESS ? 1 : 0;
    }

    /** Cancels the entry point {@code name}'s job, if one is scheduled. */
    public static void cancel(Context context, String name) {
        JobScheduler scheduler =
                (JobScheduler) context.getSystemService(Context.JOB_SCHEDULER_SERVICE);
        if (scheduler != null) {
            scheduler.cancel(jobId(name));
        }
    }

    /** Whether the entry point {@code name} has a job scheduled. */
    public static boolean scheduled(Context context, String name) {
        JobScheduler scheduler =
                (JobScheduler) context.getSystemService(Context.JOB_SCHEDULER_SERVICE);
        if (scheduler == null) {
            return false;
        }
        int id = jobId(name);
        for (JobInfo job : scheduler.getAllPendingJobs()) {
            if (job.getId() == id) {
                return true;
            }
        }
        return false;
    }

    /**
     * Posts a notification from a background job: 1 posted, 0 no notification service, -1
     * notifications not allowed (Android 13 and later ask the person, which only the
     * application's window can do). A tap opens the application.
     */
    public static int notify(Context context, int id, String title, String body) {
        NotificationManager manager =
                (NotificationManager) context.getSystemService(Context.NOTIFICATION_SERVICE);
        if (manager == null) {
            return 0;
        }
        if (Build.VERSION.SDK_INT >= 24 && !manager.areNotificationsEnabled()) {
            return -1;
        }
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= 26) {
            if (manager.getNotificationChannel(CHANNEL) == null) {
                manager.createNotificationChannel(new NotificationChannel(
                        CHANNEL, "Background", NotificationManager.IMPORTANCE_DEFAULT));
            }
            builder = new Notification.Builder(context, CHANNEL);
        } else {
            builder = new Notification.Builder(context);
        }
        int icon = context.getApplicationInfo().icon;
        builder.setSmallIcon(icon != 0 ? icon : android.R.drawable.stat_notify_sync)
                .setContentTitle(title)
                .setContentText(body)
                .setAutoCancel(true);
        Intent launch =
                context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
        if (launch != null) {
            launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_SINGLE_TOP);
            int flags = PendingIntent.FLAG_UPDATE_CURRENT;
            if (Build.VERSION.SDK_INT >= 23) {
                flags |= PendingIntent.FLAG_IMMUTABLE;
            }
            builder.setContentIntent(PendingIntent.getActivity(context, id, launch, flags));
        }
        manager.notify(CHANNEL, id, builder.build());
        return 1;
    }
}
