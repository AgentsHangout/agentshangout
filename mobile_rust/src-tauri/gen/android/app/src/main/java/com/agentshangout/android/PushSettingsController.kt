package com.agentshangout.android

import android.content.Intent
import android.os.Build
import android.provider.Settings
import android.webkit.WebView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.widget.SwitchCompat
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import org.json.JSONObject

internal class PushSettingsController(private val activity: MainActivity, private val askPermission: () -> Unit) {
    private var dialog: AlertDialog? = null
    private var status: TextView? = null
    private var master: SwitchCompat? = null
    private var updating = false
    private var configured = false
    private var owner = ""
    private var generation = 0
    private var enablingAfterPermission = false

    fun attach(webView: WebView) {
        if (BuildConfig.AGENTSHANGOUT_URL.trimEnd('/') != PushNotifications.ORIGIN ||
            !WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return
        WebViewCompat.addWebMessageListener(webView, "AgentsHangoutNotifications", setOf(PushNotifications.ORIGIN)) {
                _, message, origin, mainFrame, reply ->
            if (!mainFrame || origin.toString().trimEnd('/') != PushNotifications.ORIGIN) return@addWebMessageListener
            val request = runCatching { JSONObject(message.data ?: "") }.getOrNull() ?: return@addWebMessageListener
            when (request.optString("action")) {
                "openSettings" -> show()
                "signedOut" -> { generation++; PushNotifications.disable(activity, true); refresh() }
                "status" -> Unit
                else -> return@addWebMessageListener
            }
            val p = PushNotifications.policy(activity)
            reply.postMessage(JSONObject().put("id", request.opt("id")).put("available", true)
                .put("configured", configured && PushNotifications.configured())
                .put("enabled", p.enabled && PushNotifications.permission(activity))
                .put("permission", if (PushNotifications.permission(activity)) "granted" else "denied")
                .put("mentions", p.mentions).put("requests", p.requests)
                .put("announcements", p.announcements).toString())
        }
    }

    fun resume() {
        if (PushNotifications.policy(activity).enabled && !PushNotifications.permission(activity)) {
            generation++; PushNotifications.disable(activity)
        }
        refresh()
        val current = PushNotifications.policy(activity)
        if (current.enabled) {
            val attempt = generation
            PushNotifications.async({ PushNotifications.api("GET", "/api/push/status") }) { result ->
                activity.runOnUiThread {
                    if (attempt != generation) return@runOnUiThread
                    if (result.isFailure) {
                        if ((result.exceptionOrNull() as? PushApiException)?.code in listOf(401, 403)) {
                            generation++; PushNotifications.disable(activity, true); refresh(result.exceptionOrNull()?.message)
                        }
                        return@runOnUiThread
                    }
                    val remote = result.getOrThrow()
                    if (remote.optString("owner_id") != current.ownerId) {
                        generation++; PushNotifications.disable(activity, true); refresh()
                    } else {
                        configured = remote.optBoolean("configured")
                        owner = current.ownerId
                        if (configured) sync(PushNotifications.policy(activity))
                        refresh()
                    }
                }
            }
        }
    }
    fun permissionResult(granted: Boolean) {
        if (enablingAfterPermission && granted) enable()
        else refresh("Android notifications are off. You can change this in Android settings.")
        enablingAfterPermission = false
    }
    private fun refresh(message: String? = null) {
        val p = PushNotifications.policy(activity)
        updating = true
        master?.isChecked = p.enabled && PushNotifications.permission(activity)
        // Turning off must remain possible even if server availability/configuration changed.
        master?.isEnabled = p.enabled || (configured && PushNotifications.configured())
        updating = false
        status?.text = message ?: when {
            !PushNotifications.configured() -> "Live notifications are not configured in this app build yet."
            !configured -> "Live notifications are unavailable. Sign in and try again once the server is configured."
            !PushNotifications.permission(activity) -> "Notifications are off in Android. Turn on notifications here to allow them."
            p.enabled -> "Live notifications are on for this phone."
            else -> "Notifications are off for this phone."
        }
    }
    private fun show() {
        if (dialog?.isShowing == true) return
        val layout = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            val pad = (24 * resources.displayMetrics.density).toInt()
            setPadding(pad, pad / 2, pad, 0)
        }
        status = TextView(activity).also { layout.addView(it) }
        val p = PushNotifications.policy(activity)
        master = SwitchCompat(activity).apply {
            text = "Notifications on this phone"
            setOnCheckedChangeListener { _, checked ->
                if (!updating) {
                    generation++
                    if (checked) {
                        if (!PushNotifications.permission(activity)) {
                            enablingAfterPermission = true
                            if (Build.VERSION.SDK_INT >= 33) askPermission() else openSystemSettings()
                            refresh()
                        } else enable()
                    } else { PushNotifications.disable(activity); refresh() }
                }
            }
        }.also { layout.addView(it) }
        listOf("mentions" to "@mentions", "requests" to "Requests needing my response",
            "announcements" to "Admin announcements").forEach { (key, label) ->
            val toggle = SwitchCompat(activity).apply {
                text = label
                isChecked = when (key) { "mentions" -> p.mentions; "requests" -> p.requests; else -> p.announcements }
                setOnCheckedChangeListener { _, checked ->
                    val old = PushNotifications.policy(activity)
                    val next = when (key) { "mentions" -> old.copy(mentions = checked)
                        "requests" -> old.copy(requests = checked); else -> old.copy(announcements = checked) }
                    PushNotifications.save(activity, next)
                    if (next.enabled) sync(next)
                }
            }
            layout.addView(toggle)
        }
        dialog = AlertDialog.Builder(activity).setTitle("Phone notifications").setView(layout)
            .setPositiveButton("Done", null).setNeutralButton("Android settings") { _, _ -> openSystemSettings() }
            .setNegativeButton("Send test", null).create().also { d ->
                d.setOnShowListener { d.getButton(AlertDialog.BUTTON_NEGATIVE).setOnClickListener {
                    if (!PushNotifications.policy(activity).enabled) { refresh("Turn notifications on before sending a test."); return@setOnClickListener }
                    PushNotifications.async({ PushNotifications.api("POST", "/api/push/test",
                        JSONObject().put("device_id", PushNotifications.deviceId(activity))) }) { result ->
                        activity.runOnUiThread { refresh(if (result.isSuccess) "Test sent. Watch for a phone notification."
                            else result.exceptionOrNull()?.message) }
                    }
                } }
                d.show()
            }
        refresh("Checking live notification availability…")
        PushNotifications.async({ PushNotifications.api("GET", "/api/push/status") }) { result ->
            activity.runOnUiThread {
                configured = result.getOrNull()?.optBoolean("configured") == true
                owner = result.getOrNull()?.optString("owner_id").orEmpty()
                val saved = PushNotifications.policy(activity)
                val unauthorized = (result.exceptionOrNull() as? PushApiException)?.code in listOf(401, 403)
                if (saved.enabled && (unauthorized || (result.isSuccess && (owner.isBlank() || owner != saved.ownerId)))) {
                    generation++; PushNotifications.disable(activity, true)
                }
                refresh(result.exceptionOrNull()?.message)
            }
        }
    }
    private fun enable() {
        if (!configured || owner.isBlank() || !PushNotifications.configured() || !PushNotifications.permission(activity)) { refresh(); return }
        val attempt = generation
        master?.isEnabled = false
        status?.text = "Connecting this phone…"
        val messaging = PushNotifications.messaging(activity) ?: run { refresh("Could not initialize phone notifications."); return }
        // Token creation is explicit; background auto-init stays off until registration succeeds.
        // HTTP v1 still supports registration tokens. FID registration requires a separate server migration.
        messaging.token.addOnCompleteListener { task ->
            if (attempt != generation) return@addOnCompleteListener
            if (!task.isSuccessful) { refresh("Could not connect to Google Play services. Please try again."); return@addOnCompleteListener }
            val next = PushNotifications.policy(activity).copy(enabled = true, ownerId = owner)
            PushNotifications.async({ PushNotifications.register(activity, task.result, next) }) { result ->
                activity.runOnUiThread {
                    if (attempt != generation) return@runOnUiThread
                    if (result.isSuccess && result.getOrNull()?.optString("owner_id") == owner && PushNotifications.permission(activity)) {
                        val latest = PushNotifications.policy(activity).copy(enabled = true, ownerId = owner)
                        PushNotifications.save(activity, latest)
                        messaging.isAutoInitEnabled = true
                        if (latest != next) sync(latest)
                        refresh()
                    } else { PushNotifications.disable(activity); refresh(result.exceptionOrNull()?.message ?: "Could not enable notifications for this account.") }
                }
            }
        }
    }
    private fun sync(p: PushPolicy) {
        val token = PushNotifications.token(activity) ?: return
        PushNotifications.async({
            val current = PushNotifications.policy(activity)
            if (current.enabled && current.ownerId == p.ownerId && PushNotifications.permission(activity))
                PushNotifications.register(activity, token, current)
        }) { result ->
            activity.runOnUiThread { if (result.isFailure) refresh("Saved on this phone. Server sync failed; reopen settings to try again.") }
        }
    }
    private fun openSystemSettings() {
        activity.startActivity(Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
            .putExtra(Settings.EXTRA_APP_PACKAGE, activity.packageName))
    }
    fun destroy() { generation++; dialog?.dismiss(); dialog = null; master = null; status = null }
}
