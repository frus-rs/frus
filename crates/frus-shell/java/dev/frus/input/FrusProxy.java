package dev.frus.input;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;

/**
 * Java calling Rust (milestone 839): the one {@link InvocationHandler} behind every listener a
 * Rust crate implements with {@code frus::platform::android::JavaProxy}.
 *
 * <p>{@link Proxy} implements any Java interface at run time; this class is the handler each
 * such proxy is given. It only forwards: every call goes to {@link #nativeInvoke} with the
 * number the Rust side keeps its closure under. A closure that is gone (its {@code JavaProxy}
 * dropped) answers {@code null}, which becomes the return type's default here, so a listener
 * still registered somewhere never throws into the system's code.
 *
 * <p>{@link Receiver} is the same forwarding for broadcasts: {@code BroadcastReceiver} is a
 * class, which a proxy cannot extend, so it is the one class written out.
 */
public final class FrusProxy implements InvocationHandler {
    private final long id;

    private FrusProxy(long id) {
        this.id = id;
    }

    /** A proxy implementing {@code interfaces}, every call forwarded to closure {@code id}. */
    public static Object create(ClassLoader loader, Class<?>[] interfaces, long id) {
        return Proxy.newProxyInstance(loader, interfaces, new FrusProxy(id));
    }

    /** A broadcast receiver whose {@code onReceive} is forwarded to closure {@code id}. */
    public static BroadcastReceiver receiver(long id) {
        return new Receiver(id);
    }

    static native Object nativeInvoke(long id, Object proxy, String method, Object[] args);

    @Override
    public Object invoke(Object proxy, Method method, Object[] args) {
        // The three methods of Object a proxy also receives: answered here, as identity, so a
        // proxy works as a key and in a listener list without a round trip to Rust.
        if (method.getDeclaringClass() == Object.class) {
            String name = method.getName();
            if (name.equals("equals")) {
                return args != null && args.length == 1 && proxy == args[0];
            }
            if (name.equals("hashCode")) {
                return System.identityHashCode(proxy);
            }
            if (name.equals("toString")) {
                return "FrusProxy#" + id;
            }
        }
        Object result = nativeInvoke(id, proxy, method.getName(), args == null ? new Object[0] : args);
        return result != null ? result : defaultFor(method.getReturnType());
    }

    /** What a method returning {@code type} answers when Rust answered nothing. */
    static Object defaultFor(Class<?> type) {
        if (!type.isPrimitive() || type == void.class) {
            return null;
        }
        if (type == boolean.class) {
            return Boolean.FALSE;
        }
        if (type == char.class) {
            return (char) 0;
        }
        if (type == byte.class) {
            return (byte) 0;
        }
        if (type == short.class) {
            return (short) 0;
        }
        if (type == int.class) {
            return 0;
        }
        if (type == long.class) {
            return 0L;
        }
        if (type == float.class) {
            return 0f;
        }
        return 0d;
    }

    /** {@code onReceive(context, intent)}, forwarded under the method name "onReceive". */
    static final class Receiver extends BroadcastReceiver {
        private final long id;

        Receiver(long id) {
            this.id = id;
        }

        @Override
        public void onReceive(Context context, Intent intent) {
            nativeInvoke(id, this, "onReceive", new Object[] {context, intent});
        }
    }
}
