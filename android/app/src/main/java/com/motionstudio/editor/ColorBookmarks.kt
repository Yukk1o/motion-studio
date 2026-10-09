package com.motionstudio.editor

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

internal data class ColorBookmark(val id: String, val name: String, val color: Rgba)

internal val builtInColors = listOf(
    "#FFFFFF", "#000000", "#FF5555", "#FF9F50", "#FFD600", "#60D98B",
    "#54DCC7", "#5DA9F6", "#9C7CF4", "#FF6CAB", "#A77863", "#7C8796",
).map { Rgba.hex(it, 1.0, true)!! }

/** User preferences, shared by projects. A pin refers to a favorite's stable ID. */
internal class ColorBookmarks(context: Context, storageName: String = "motion-color-palette") {
    private val prefs = context.applicationContext.getSharedPreferences(storageName, Context.MODE_PRIVATE)
    private val defaultCommon = listOf(
        "builtin:#FFFFFF", "builtin:#000000", "builtin:#FFD600",
        "builtin:#54DCC7", "builtin:#5DA9F6", "builtin:#FF5555",
    )

    fun list(): List<ColorBookmark> = runCatching {
        JSONArray(prefs.getString("favorites", "[]")).objects().mapNotNull { entry ->
            runCatching {
                val color = Rgba.from(entry.getJSONArray("rgba"))
                val id = entry.getString("id").takeIf { it.isNotBlank() } ?: return@mapNotNull null
                if ((0..3).any { !color.component(it).isFinite() || color.component(it) !in 0.0..1.0 }) null
                else ColorBookmark(id, entry.getString("name"), color)
            }.getOrNull()
        }.distinctBy { it.id }
    }.getOrDefault(emptyList())

    private fun write(values: List<ColorBookmark>) {
        val entries = values.map {
            JSONObject().put("id", it.id).put("name", it.name).put("rgba", it.color.array())
        }
        prefs.edit().putString("favorites", JSONArray(entries).toString()).apply()
    }

    fun add(name: String, color: Rgba) {
        require((0..3).all { color.component(it).isFinite() && color.component(it) in 0.0..1.0 })
        write(list() + ColorBookmark(UUID.randomUUID().toString(), name.trim().ifBlank { color.hex() }.take(80), color))
    }

    fun rename(id: String, name: String) {
        write(list().map {
            if (it.id == id) it.copy(name = name.trim().ifBlank { it.color.hex() }.take(80)) else it
        })
    }

    fun remove(id: String) {
        val pins = commonIds().filter { it != "favorite:$id" }
        write(list().filter { it.id != id })
        setCommon(pins)
    }

    private fun resolve(id: String, favorites: List<ColorBookmark>): Rgba? = when {
        id.startsWith("builtin:") -> builtInColors.firstOrNull { "builtin:${it.hex()}" == id }
        id.startsWith("favorite:") -> favorites.firstOrNull { "favorite:${it.id}" == id }?.color
        else -> null
    }

    fun commonIds(): List<String> {
        val favorites = list()
        return runCatching {
            val values = JSONArray(prefs.getString("common", null) ?: JSONArray(defaultCommon).toString())
            (0 until values.length()).mapNotNull { values.optString(it).takeIf { id -> resolve(id, favorites) != null } }.distinct()
        }.getOrDefault(emptyList())
    }

    fun setCommon(ids: List<String>) {
        val favorites = list()
        val valid = ids.distinct().filter { resolve(it, favorites) != null }
        prefs.edit().putString("common", JSONArray(valid).toString()).apply()
    }

    fun common(): List<Pair<String, Rgba>> {
        val favorites = list()
        return commonIds().mapNotNull { id -> resolve(id, favorites)?.let { id to it } }
    }

    fun toggleCommon(id: String) {
        val ids = commonIds()
        setCommon(if (id in ids) ids - id else ids + id)
    }

    fun moveCommon(id: String, delta: Int) {
        val ids = commonIds().toMutableList()
        val old = ids.indexOf(id)
        val next = old + delta
        if (old >= 0 && next in ids.indices) {
            ids.removeAt(old)
            ids.add(next, id)
            setCommon(ids)
        }
    }
}
