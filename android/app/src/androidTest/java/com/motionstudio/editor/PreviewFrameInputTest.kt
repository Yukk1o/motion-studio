package com.motionstudio.editor

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class PreviewFrameInputTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    @Test fun staleOrInvalidRenderFramesDoNotPoisonProjectStateOrFollowingEdits() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(app.filesDir,"acceptance/frame-input-"+UUID.randomUUID()).apply{mkdirs()}
        val session=NativeBridge.create(root.absolutePath,data(NativeBridge.projectTemplate(0)).toString())
        assertTrue(session>0)
        try {
            data(NativeBridge.seek(session,12.0))
            for(frame in listOf(-.25,Double.NaN,Double.POSITIVE_INFINITY,180.0,360.0)) {
                assertFalse(NativeBridge.render(session,frame))
                val state=data(NativeBridge.state(session))
                File(root,"invalid-frame-state.json").writeText(state.toString(2))
                assertEquals("Rejected render changed the edit frame",12.0,state.getDouble("frame"),.0001)
                assertTrue("Rejected time was reported as a GPU failure",state.isNull("renderError"))
                val command=JSONObject().put("op","rename").put("object",2).put("name","仍可编辑")
                data(NativeBridge.command(session,command.toString()))
            }
            NativeBridge.render(session,179.5)
            assertEquals(179.5,data(NativeBridge.state(session)).getDouble("frame"),.0001)
        }finally{NativeBridge.destroy(session)}
    }
}
