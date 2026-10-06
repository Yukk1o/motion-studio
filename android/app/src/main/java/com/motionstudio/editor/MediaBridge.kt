package com.motionstudio.editor

import android.content.Context
import java.nio.ByteBuffer

object MediaBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun request(id:Long,context:Context?,request:String):String
    @JvmStatic external fun readPcmInto(id:Long,startSample:Long,frames:Int,output:ByteBuffer):String
    @JvmStatic external fun freezeAudio(id:Long):String
    @JvmStatic external fun readFrozenPcmInto(handle:Long,startSample:Long,frames:Int,output:ByteBuffer):String
    @JvmStatic external fun releaseFrozenAudio(handle:Long):String
    @JvmStatic external fun readVideoFrameInto(id:Long,objectId:Long,sequence:Long,output:ByteBuffer):String
    @JvmStatic external fun freezeVideo(id:Long):String
    @JvmStatic external fun requestFrozenVideoFrame(handle:Long,objectId:Long,frame:Double,sequence:Long):String
    @JvmStatic external fun readFrozenVideoFrameInto(handle:Long,objectId:Long,sequence:Long,output:ByteBuffer):String
    @JvmStatic external fun releaseFrozenVideo(handle:Long):String
    @JvmStatic external fun requestFrozenCompositionFrame(handle:Long,frame:Double,sequence:Long):String
    @JvmStatic external fun readFrozenCompositionVideoInto(handle:Long,objectId:Long,sequence:Long,output:ByteBuffer):String
}

internal fun nativeData(raw:String):org.json.JSONObject {
    val result=org.json.JSONObject(raw)
    check(result.optBoolean("ok")){result.optString("error","操作失败")}
    return result.getJSONObject("data")
}
