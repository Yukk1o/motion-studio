package com.motionstudio.editor

import android.view.Surface
import java.nio.ByteBuffer

object NativeBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun create(root: String, project: String): Long
    @JvmStatic external fun projectTemplate(kind:Int):String
    @JvmStatic external fun curveGraph(easing:String):String
    @JvmStatic external fun creationError():String
    @JvmStatic external fun newProject(id:Long,project:String):String
    @JvmStatic external fun openProject(id:Long,directory:String):String
    @JvmStatic external fun state(id: Long): String
    @JvmStatic external fun resourceInfo(projectDirectory:String):String
    @JvmStatic external fun command(id: Long, json: String): String
    @JvmStatic external fun drag(id: Long, objectId: Long, dx: Double, dy: Double, width: Int, height: Int): String
    @JvmStatic external fun history(id: Long, operation: Int): String
    @JvmStatic external fun seek(id: Long, frame: Double): String
    @JvmStatic external fun observe(id: Long, enabled: Boolean, azimuth: Double, elevation: Double): String
    @JvmStatic external fun navigate(id: Long, dx: Double, dy: Double, zoom: Double, multi: Boolean, width: Int, height: Int): String
    @JvmStatic external fun view(id: Long, kind: Int): String
    @JvmStatic external fun save(id: Long): String
    @JvmStatic external fun surface(id: Long, surface: Surface?, width: Int, height: Int): String
    @JvmStatic external fun render(id: Long, frame: Double): Boolean
    @JvmStatic external fun previewInfo(id:Long):String
    @JvmStatic external fun previewMode(id:Long,mode:Int,thermal:Int):String
    @JvmStatic external fun startProfiling(id:Long,maxFrames:Int):String
    @JvmStatic external fun stopProfiling(id:Long):String
    @JvmStatic external fun injectGraphicsFault(id:Long,kind:Int):String
    @JvmStatic external fun capture(id: Long): String
    @JvmStatic external fun pack(id: Long): String
    @JvmStatic external fun replace(id: Long, project: String): String
    @JvmStatic external fun importProject(id: Long, path: String): String
    @JvmStatic external fun sampleInto(id: Long, frame: Int, buffer: ByteBuffer): Int
    @JvmStatic external fun assetPixels(id: Long, asset: Long): ByteArray?
    @JvmStatic external fun assetPixelsInto(id: Long, asset: Long, buffer: ByteBuffer): String
    /** Stateless header inspection, safe on the URI import I/O thread. */
    @JvmStatic external fun imageInfo(path: String): String
    /** Decode validation and proxy warming; stateless, call on an I/O worker. */
    @JvmStatic external fun prepareImage(root: String, path: String): String
    @JvmStatic external fun plugin(id:Long,request:String):String
    @JvmStatic external fun renderPlanInfo(id:Long):String
    @JvmStatic external fun sampleRenderPlanInto(id:Long,frame:Int,buffer:ByteBuffer):Int
    @JvmStatic external fun pluginPixels(id:Long,program:Int,resource:Int):ByteArray?
    @JvmStatic external fun destroy(id: Long)
}
