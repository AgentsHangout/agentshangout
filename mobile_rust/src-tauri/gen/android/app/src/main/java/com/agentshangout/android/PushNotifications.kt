package com.agentshangout.android

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.net.Uri
import android.webkit.CookieManager
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import com.google.firebase.FirebaseApp
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.UUID
import java.util.concurrent.Executors

internal object PushNotifications {
    const val ORIGIN = "https://agentshangout.com"
    const val PATH_EXTRA = "phone_notification_path"
    private val worker = Executors.newSingleThreadExecutor()
    private fun prefs(c: Context) = c.getSharedPreferences("phone_push", Context.MODE_PRIVATE)
    fun configured() = BuildConfig.AGENTSHANGOUT_URL.trimEnd('/') == ORIGIN &&
        listOf(BuildConfig.FIREBASE_APP_ID, BuildConfig.FIREBASE_PROJECT_ID,
            BuildConfig.FIREBASE_API_KEY, BuildConfig.FIREBASE_SENDER_ID).all { it.isNotBlank() }
    fun policy(c: Context): PushPolicy = prefs(c).let {
        PushPolicy(it.getBoolean("enabled", false), it.getBoolean("mentions", true),
            it.getBoolean("requests", true), it.getBoolean("announcements", true),
            it.getString("owner_id", "") ?: "")
    }
    @Synchronized fun save(c: Context, p: PushPolicy) {
        prefs(c).edit().putBoolean("enabled", p.enabled).putBoolean("mentions", p.mentions)
            .putBoolean("requests", p.requests).putBoolean("announcements", p.announcements)
            .putString("owner_id", p.ownerId).apply()
    }
    fun permission(c: Context) = NotificationManagerCompat.from(c).areNotificationsEnabled()
    fun deviceId(c: Context): String = synchronized(this) {
        prefs(c).getString("device_id", null) ?: UUID.randomUUID().toString().also {
            prefs(c).edit().putString("device_id", it).apply()
        }
    }
    fun messaging(c: Context): FirebaseMessaging? {
        if (!configured()) return null
        if (FirebaseApp.getApps(c).isEmpty()) FirebaseApp.initializeApp(c)
        return runCatching { FirebaseMessaging.getInstance() }.getOrNull()
    }
    @Synchronized fun disable(c: Context, signedOut: Boolean = false) {
        val p = policy(c)
        save(c, p.copy(enabled = false, ownerId = if (signedOut) "" else p.ownerId))
        messaging(c)?.isAutoInitEnabled = false
        NotificationManagerCompat.from(c).cancelAll()
        async({ api("DELETE", "/api/push/devices/${deviceId(c)}") }) {}
    }
    fun api(method: String, path: String, body: JSONObject? = null): JSONObject {
        val connection = URL(ORIGIN + path).openConnection() as HttpURLConnection
        try {
            connection.requestMethod = method
            connection.connectTimeout = 10_000
            connection.readTimeout = 10_000
            connection.instanceFollowRedirects = false
            connection.setRequestProperty("Origin", ORIGIN)
            connection.setRequestProperty("Accept", "application/json")
            CookieManager.getInstance().getCookie(ORIGIN)?.let { connection.setRequestProperty("Cookie", it) }
            if (body != null) {
                connection.doOutput = true
                connection.setRequestProperty("Content-Type", "application/json")
                connection.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            }
            val code = connection.responseCode
            if (code !in 200..299) throw PushApiException(code, when (code) {
                401, 403 -> "Sign in to your personal account to enable notifications."
                503 -> "Live notifications are not configured on the server yet."
                else -> "Could not save notification settings. Please try again."
            })
            return connection.inputStream.bufferedReader().use { JSONObject(it.readText().ifBlank { "{}" }) }
        } finally { connection.disconnect() }
    }
    fun register(c: Context, token: String, p: PushPolicy): JSONObject {
        val response = api("PUT", "/api/push/devices/${deviceId(c)}", JSONObject()
            .put("token", token).put("enabled", p.enabled).put("mentions", p.mentions)
            .put("requests", p.requests).put("announcements", p.announcements))
        prefs(c).edit().putString("token", token).apply()
        return response
    }
    fun token(c: Context) = prefs(c).getString("token", null)
    fun <T> async(task: () -> T, done: (Result<T>) -> Unit) { worker.execute { done(runCatching(task)) } }
    fun receive(c: Context, data: Map<String, String>) = synchronized(this) {
        val p = policy(c)
        if (!permission(c)) { if (p.enabled) disable(c); return@synchronized }
        if (!p.accepts(data["category"], data["owner_id"], true)) return@synchronized
        val id = data["id"]?.takeIf { it.length in 1..200 } ?: return@synchronized
        val path = PushPolicy.safePath(data["path"]) ?: return@synchronized
        val recent = runCatching { JSONArray(prefs(c).getString("recent", "[]")) }.getOrDefault(JSONArray())
        val ids = (0 until recent.length()).map { recent.getString(it) }
        if (id in ids) return@synchronized
        val category = data["category"]!!
        val channel = "hangout_$category"
        val label = when (category) { "mention" -> "Mentions"; "request" -> "Requests for your response";
            "announcement" -> "Server announcements"; else -> "Test notifications" }
        if (Build.VERSION.SDK_INT >= 26) c.getSystemService(NotificationManager::class.java)
            .createNotificationChannel(NotificationChannel(channel, label, NotificationManager.IMPORTANCE_DEFAULT))
        val intent = Intent(c, MainActivity::class.java).putExtra(PATH_EXTRA, path)
            // Intent data participates in PendingIntent identity; hash collisions must not replace another room's tap.
            .setData(Uri.Builder().scheme("agentshangout-notification").authority("open").appendPath(id).build())
            .setFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        val pending = PendingIntent.getActivity(c, id.hashCode(), intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val title = when (category) { "mention" -> "You were mentioned"; "request" -> "Your response is needed";
            "test" -> "Notifications are working"; else -> data["title"].orEmpty().take(80).ifBlank { "Server announcement" } }
        val body = when (category) { "mention" -> "Open Agents Hangout to view the message."
            "request" -> "Open Agents Hangout to review the request."
            "test" -> "This is a live notification from Agents Hangout."
            else -> data["body"].orEmpty().take(300) }
        val publicVersion = NotificationCompat.Builder(c, channel).setSmallIcon(R.drawable.ic_stat_hangout)
            .setContentTitle("Agents Hangout").setContentText("You have a new notification.").build()
        val notification = NotificationCompat.Builder(c, channel).setSmallIcon(R.drawable.ic_stat_hangout)
            .setContentTitle(title).setContentText(body).setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setContentIntent(pending).setAutoCancel(true).setOnlyAlertOnce(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE).setPublicVersion(publicVersion).build()
        runCatching { NotificationManagerCompat.from(c).notify(id, 0, notification) }.onSuccess {
            prefs(c).edit().putString("recent", JSONArray(PushPolicy.recent(ids, id)).toString()).apply()
        }
    }
}

internal class PushApiException(val code: Int, message: String) : IllegalStateException(message)

class PushMessagingService : FirebaseMessagingService() {
    override fun onMessageReceived(message: RemoteMessage) { PushNotifications.receive(this, message.data) }
    override fun onNewToken(token: String) {
        val p = PushNotifications.policy(this)
        if (p.enabled && PushNotifications.permission(this)) PushNotifications.async({
            val current = PushNotifications.policy(this)
            if (!current.enabled || current.ownerId != p.ownerId || !PushNotifications.permission(this)) return@async
            val result = PushNotifications.register(this, token, current)
            if (result.optString("owner_id") != p.ownerId) PushNotifications.disable(this, true)
        }) {}
    }
}
