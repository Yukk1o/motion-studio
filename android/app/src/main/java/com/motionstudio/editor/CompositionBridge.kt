package com.motionstudio.editor

import android.content.Context
import org.json.JSONObject
import java.nio.ByteBuffer

/** Composition API v1. Call on the engine owner thread. This adapter owns no UI. */
object CompositionBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun request(id:Long,request:String):String
    /** Returns written bytes, -requiredBytes for growth, or -1 on error. */
    @JvmStatic external fun sampleFrameBundleInto(id:Long,composition:String,frame:Double,buffer:ByteBuffer):Int
    @JvmStatic external fun render(id:Long,composition:String,frame:Double):Boolean
    private fun requireContext(id:Long,composition:String) {
        nativeData(request(id,JSONObject().put("version",1).put("composition",composition).put("op","assert_context").toString()))
    }
    fun command(id:Long,composition:String,command:JSONObject):String {
        requireContext(id,composition)
        return NativeBridge.command(id,command.put("composition",composition).toString())
    }
    fun media(id:Long,composition:String,context:Context?,request:JSONObject):String {
        requireContext(id,composition)
        return MediaBridge.request(id,context,request.put("composition",composition).toString())
    }
    fun plugin(id:Long,composition:String,request:JSONObject):String {
        requireContext(id,composition)
        return NativeBridge.plugin(id,request.put("composition",composition).toString())
    }
    fun capture(id:Long,composition:String):String {requireContext(id,composition);return NativeBridge.capture(id)}
    fun seek(id:Long,composition:String,frame:Double):String = request(id,JSONObject().put("version",1).put("composition",composition).put("op","seek").put("frame",frame).toString())
    fun drag(id:Long,composition:String,objectId:Long,dx:Double,dy:Double,width:Int,height:Int):String {requireContext(id,composition);return NativeBridge.drag(id,objectId,dx,dy,width,height)}
    fun readPcmInto(id:Long,composition:String,startSample:Long,frames:Int,buffer:ByteBuffer):String {requireContext(id,composition);return MediaBridge.readPcmInto(id,startSample,frames,buffer)}
    fun freezeAudio(id:Long,composition:String):String {requireContext(id,composition);return MediaBridge.freezeAudio(id)}
    fun freezeVideo(id:Long,composition:String):String {requireContext(id,composition);return MediaBridge.freezeVideo(id)}
    fun history(id:Long,composition:String,operation:Int):String {requireContext(id,composition);return NativeBridge.history(id,operation)}
    /** Copy this immutable JSON to VideoExporter; it includes the complete graph and shared assets. */
    fun freezeProject(id:Long,composition:String):String {requireContext(id,composition);return nativeData(NativeBridge.state(id)).getJSONObject("project").toString()}
}
