package com.motionstudio.editor

import android.app.ActivityManager
import android.content.Context
import org.json.JSONObject

/** Physical RAM informs a host policy; it is not available GPU memory. */
data class DeviceMemoryProfile(val totalMem:Long=0,val guarded:Boolean=true) {
    internal fun applyTo(native:Long) {
        val result=JSONObject(NativeBridge.configureMemory(native,totalMem,guarded))
        check(result.optBoolean("ok")){result.optString("error","设备内存策略配置失败")}
    }
    companion object {
        fun read(context:Context):DeviceMemoryProfile=runCatching {
            val manager=context.getSystemService(ActivityManager::class.java)
                ?:return@runCatching DeviceMemoryProfile()
            val info=ActivityManager.MemoryInfo().also{manager.getMemoryInfo(it)}
            DeviceMemoryProfile(info.totalMem.coerceAtLeast(0),manager.isLowRamDevice||info.lowMemory)
        }.getOrDefault(DeviceMemoryProfile())
    }
}
