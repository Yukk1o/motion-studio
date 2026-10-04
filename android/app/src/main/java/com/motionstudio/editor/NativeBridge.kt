package com.motionstudio.editor

import android.view.Surface
import java.nio.ByteBuffer

object NativeBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun create(root: String, project: String): Long
    @JvmStatic external fun state(id: Long): String
    @JvmStatic external fun command(id: Long, json: String): String
    @JvmStatic external fun drag(id: Long, objectId: Long, dx: Double, dy: Double, width: Int, height: Int): String
    @JvmStatic external fun history(id: Long, operation: Int): String
    @JvmStatic external fun seek(id: Long, frame: Double): String
    @JvmStatic external fun observe(id: Long, enabled: Boolean, azimuth: Double, elevation: Double): String
    @JvmStatic external fun view(id: Long, kind: Int): String
    @JvmStatic external fun save(id: Long): String
    @JvmStatic external fun surface(id: Long, surface: Surface?, width: Int, height: Int): String
    @JvmStatic external fun render(id: Long, frame: Double): Boolean
    @JvmStatic external fun capture(id: Long): String
    @JvmStatic external fun pack(id: Long): String
    @JvmStatic external fun replace(id: Long, project: String): String
    @JvmStatic external fun importProject(id: Long, path: String): String
    @JvmStatic external fun sampleInto(id: Long, frame: Int, buffer: ByteBuffer): Int
    @JvmStatic external fun assetPixels(id: Long, asset: Long): ByteArray?
    @JvmStatic external fun destroy(id: Long)
}
