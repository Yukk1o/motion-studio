package com.motionstudio.editor

import android.content.Context
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.After
import org.junit.Assert.*
import org.junit.Test
import java.util.UUID

class ColorBookmarksTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private val storage = "color-bookmarks-test-${UUID.randomUUID()}"

    @After fun cleanup() {
        context.getSharedPreferences(storage, Context.MODE_PRIVATE).edit().clear().commit()
    }

    @Test fun customPinsKeepOrderNamesAndAlphaWhenStorageIsReopened() {
        val store = ColorBookmarks(context, storage)
        val color = Rgba(.123456, .5, .75, .37)
        store.add("轨迹蓝", color)
        val favorite = store.list().single()
        val pin = "favorite:${favorite.id}"
        store.setCommon(listOf("builtin:#FFD600", pin, "builtin:#000000", pin, "unknown"))
        store.moveCommon(pin, -1)
        store.rename(favorite.id, "粒子蓝")

        // commit waits for preceding apply writes to reach the preferences file.
        assertTrue(context.getSharedPreferences(storage, Context.MODE_PRIVATE).edit().commit())
        val restored = ColorBookmarks(context, storage)
        assertEquals(listOf(pin, "builtin:#FFD600", "builtin:#000000"), restored.commonIds())
        assertEquals("粒子蓝", restored.list().single().name)
        assertEquals(color, restored.common().first().second)
        restored.toggleCommon("builtin:#FFD600")
        restored.remove(favorite.id)
        assertEquals(listOf("builtin:#000000"), ColorBookmarks(context, storage).commonIds())
        assertTrue(restored.list().isEmpty())
    }

    @Test fun intentionallyEmptyCommonPaletteStaysEmptyInsteadOfRestoringDefaults() {
        val store = ColorBookmarks(context, storage)
        assertFalse(store.commonIds().isEmpty())
        store.setCommon(emptyList())
        assertTrue(ColorBookmarks(context, storage).common().isEmpty())
    }
}
