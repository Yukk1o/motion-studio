package com.motionstudio.editor

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class DeviceMemoryPolicyTest {
    private fun data(raw:String):JSONObject {
        val value=JSONObject(raw);assertTrue(value.toString(),value.getBoolean("ok"));return value.getJSONObject("data")
    }
    @Test fun nativePolicyPreviewAndSerializedGlesBudgetStayInSync() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(context.filesDir,"acceptance/memory-policy-"+UUID.randomUUID()).apply{mkdirs()}
        val project=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("layers",JSONArray())
        val native=NativeBridge.create(root.absolutePath,project.toString());assertTrue(native!=0L)
        try {
            for((gib,mib)in listOf(3L to 96L,6L to 192L,12L to 384L)) {
                val budget=mib*1024*1024
                assertEquals(budget,data(NativeBridge.configureMemory(native,gib*1024*1024*1024,false)).getLong("scratchBudgetBytes"))
                assertEquals(budget,data(NativeBridge.previewInfo(native)).getLong("scratchBudgetBytes"))
                val info=data(NativeBridge.renderPlanInfo(native))
                assertEquals(RenderPlanBudget.VERSION,info.getInt("version"));assertEquals(budget,info.getLong("scratchBudgetBytes"))
                val plan=ByteBuffer.allocateDirect(128).order(ByteOrder.nativeOrder())
                assertEquals(128,NativeBridge.sampleRenderPlanInto(native,0,plan))
                assertEquals(budget,RenderPlanBudget.scratchBytes(plan))
            }
            assertEquals(64L*1024*1024,data(NativeBridge.configureMemory(native,12L*1024*1024*1024,true)).getLong("scratchBudgetBytes"))
            assertEquals(64L*1024*1024,data(NativeBridge.configureMemory(native,0,false)).getLong("scratchBudgetBytes"))
            assertFalse(JSONObject(NativeBridge.configureMemory(native,-1,false)).getBoolean("ok"))
        } finally {NativeBridge.destroy(native)}
    }
}
