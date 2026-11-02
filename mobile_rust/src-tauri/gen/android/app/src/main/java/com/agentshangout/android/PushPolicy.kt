package com.agentshangout.android

internal data class PushPolicy(
    val enabled: Boolean = false,
    val mentions: Boolean = true,
    val requests: Boolean = true,
    val announcements: Boolean = true,
    val ownerId: String = ""
) {
    fun accepts(category: String?, owner: String?, permission: Boolean): Boolean =
        enabled && permission && ownerId.isNotEmpty() && owner == ownerId && when (category) {
            "mention" -> mentions
            "request" -> requests
            "announcement" -> announcements
            "test" -> true
            else -> false
        }

    companion object {
        fun safePath(path: String?): String? = path?.takeIf {
            it == "/" || Regex("^/h/[A-Za-z0-9_-]{1,128}$").matches(it)
        }
        fun recent(previous: List<String>, id: String): List<String> =
            (previous.filterNot { it == id } + id).takeLast(100)
    }
}
