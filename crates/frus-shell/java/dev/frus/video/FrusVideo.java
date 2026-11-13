package dev.frus.video;

import android.media.AudioAttributes;
import android.media.MediaPlayer;
import android.media.PlaybackParams;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.view.Surface;

/**
 * One clip of a video player (milestone 797): a {@link MediaPlayer} whose pictures go into the
 * surface frus gives it, and whose sound, clock, seeking and streaming are the platform's.
 *
 * <p>Everything it hears is handed to {@link #nativeEvent}: the clip is open (its duration and
 * size), the picture changed size, the position (four times a second while it plays), what is
 * downloaded, waiting for data, the end, an error. The player's callbacks run on the main
 * thread; the calls from frus come from its own thread, which {@code MediaPlayer} allows.
 *
 * <p>Event kinds: 1 open (a = duration in ms, b = width << 32 | height), 2 size (a = width,
 * b = height), 3 position (a = ms), 4 downloaded (a = percent), 5 waiting (a = 1 or 0),
 * 6 end, 7 error (a = what, b = extra).
 */
public final class FrusVideo implements MediaPlayer.OnPreparedListener,
        MediaPlayer.OnCompletionListener, MediaPlayer.OnErrorListener,
        MediaPlayer.OnInfoListener, MediaPlayer.OnBufferingUpdateListener,
        MediaPlayer.OnVideoSizeChangedListener, MediaPlayer.OnSeekCompleteListener {
    private static final String TAG = "frus-video";

    static final int OPEN = 1;
    static final int SIZE = 2;
    static final int POSITION = 3;
    static final int DOWNLOADED = 4;
    static final int WAITING = 5;
    static final int END = 6;
    static final int ERROR = 7;

    private final long handle;
    private final MediaPlayer player = new MediaPlayer();
    private final Handler main = new Handler(Looper.getMainLooper());
    private boolean released;
    private boolean prepared;
    private boolean playing;
    private float speed = 1f;

    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            int position;
            synchronized (FrusVideo.this) {
                if (released || !playing) {
                    return;
                }
                position = player.getCurrentPosition();
            }
            nativeEvent(handle, POSITION, position, 0);
            main.postDelayed(this, 250);
        }
    };

    native void nativeEvent(long handle, int kind, long a, long b);

    private FrusVideo(long handle) {
        this.handle = handle;
    }

    /**
     * Opens {@code source} (a file path or an address) and starts preparing it; the answer
     * comes as an event. Throws what {@code setDataSource} throws.
     */
    public static FrusVideo open(long handle, String source) throws Exception {
        FrusVideo video = new FrusVideo(handle);
        MediaPlayer p = video.player;
        p.setOnPreparedListener(video);
        p.setOnCompletionListener(video);
        p.setOnErrorListener(video);
        p.setOnInfoListener(video);
        p.setOnBufferingUpdateListener(video);
        p.setOnVideoSizeChangedListener(video);
        p.setOnSeekCompleteListener(video);
        p.setAudioAttributes(new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE)
                .build());
        try {
            p.setDataSource(source);
            p.prepareAsync();
        } catch (Exception e) {
            p.release();
            throw e;
        }
        return video;
    }

    /** Where the pictures go. */
    public synchronized void setSurface(Surface surface) {
        if (!released) {
            player.setSurface(surface);
        }
    }

    public synchronized void play() {
        if (released || !prepared) {
            return;
        }
        player.start();
        if (speed != 1f) {
            applySpeed();
        }
        if (!playing) {
            playing = true;
            main.post(tick);
        }
    }

    public synchronized void pause() {
        if (released || !prepared) {
            return;
        }
        playing = false;
        if (player.isPlaying()) {
            player.pause();
        }
        nativeEvent(handle, POSITION, player.getCurrentPosition(), 0);
    }

    public synchronized void seekTo(long ms) {
        if (released || !prepared) {
            return;
        }
        if (Build.VERSION.SDK_INT >= 26) {
            player.seekTo(ms, MediaPlayer.SEEK_CLOSEST);
        } else {
            player.seekTo((int) ms);
        }
    }

    public synchronized void setVolume(float volume) {
        if (!released) {
            player.setVolume(volume, volume);
        }
    }

    public synchronized void setLooping(boolean looping) {
        if (!released) {
            player.setLooping(looping);
        }
    }

    /** A speed is set on a playing clip at once; a paused one takes it when it plays. */
    public synchronized void setSpeed(float speed) {
        this.speed = speed;
        if (!released && prepared && playing) {
            applySpeed();
        }
    }

    private void applySpeed() {
        if (Build.VERSION.SDK_INT < 23) {
            Log.w(TAG, "a playback speed needs Android 6");
            return;
        }
        try {
            PlaybackParams params = player.getPlaybackParams();
            player.setPlaybackParams(params.setSpeed(speed));
        } catch (Exception e) {
            Log.w(TAG, "playback speed " + speed + " refused: " + e);
        }
    }

    /** Releases the player; nothing is heard of it any more. */
    public void release() {
        synchronized (this) {
            if (released) {
                return;
            }
            released = true;
            playing = false;
        }
        main.removeCallbacks(tick);
        player.release();
    }

    @Override
    public void onPrepared(MediaPlayer mp) {
        long duration;
        long size;
        synchronized (this) {
            if (released) {
                return;
            }
            prepared = true;
            duration = Math.max(0, mp.getDuration());
            size = ((long) mp.getVideoWidth() << 32) | (mp.getVideoHeight() & 0xffffffffL);
        }
        nativeEvent(handle, OPEN, duration, size);
    }

    @Override
    public void onVideoSizeChanged(MediaPlayer mp, int width, int height) {
        if (!released && prepared && width > 0 && height > 0) {
            nativeEvent(handle, SIZE, width, height);
        }
    }

    @Override
    public void onCompletion(MediaPlayer mp) {
        int position;
        synchronized (this) {
            if (released) {
                return;
            }
            playing = false;
            position = mp.getCurrentPosition();
        }
        main.removeCallbacks(tick);
        nativeEvent(handle, POSITION, position, 0);
        nativeEvent(handle, END, 0, 0);
    }

    @Override
    public boolean onError(MediaPlayer mp, int what, int extra) {
        synchronized (this) {
            if (released) {
                return true;
            }
            playing = false;
        }
        main.removeCallbacks(tick);
        nativeEvent(handle, ERROR, what, extra);
        return true;
    }

    @Override
    public boolean onInfo(MediaPlayer mp, int what, int extra) {
        if (released) {
            return false;
        }
        if (what == MediaPlayer.MEDIA_INFO_BUFFERING_START) {
            nativeEvent(handle, WAITING, 1, 0);
        } else if (what == MediaPlayer.MEDIA_INFO_BUFFERING_END) {
            nativeEvent(handle, WAITING, 0, 0);
        }
        return false;
    }

    @Override
    public void onBufferingUpdate(MediaPlayer mp, int percent) {
        if (!released) {
            nativeEvent(handle, DOWNLOADED, percent, 0);
        }
    }

    @Override
    public void onSeekComplete(MediaPlayer mp) {
        int position;
        synchronized (this) {
            if (released) {
                return;
            }
            position = mp.getCurrentPosition();
        }
        nativeEvent(handle, POSITION, position, 0);
    }
}
