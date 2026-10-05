package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaMetadataRetriever
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class SeparateDimensionsApiTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/axis-api-"+UUID.randomUUID()).apply{mkdirs()}
    private fun command(session:Long,op:String,vararg fields:Pair<String,Any>):JSONObject= data(NativeBridge.command(session,
        JSONObject().put("op",op).put("composition","comp-main").apply{fields.forEach{put(it.first,it.second)}}.toString()))
    private fun axes(state:JSONObject,key:String)=state.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform").getJSONObject(key).getJSONObject("axes")
    @Test fun explicitSeparationAndAxisCommandsReturnRealLocalAndCompositionTracks() {
        val root=root();val session=NativeBridge.create(root.absolutePath,LayerClipTimelineTest().fixture().toString());assertTrue(session>0)
        try {
            val original=data(NativeBridge.state(session));assertTrue(original.getJSONObject("capabilities").getJSONObject("separate_dimensions").getBoolean("supported"))
            assertFalse(original.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform").getJSONObject("rotation").has("axes"))
            val separated=command(session,"separate_dimensions","object" to 2,"property" to "rotation")
            val a=axes(separated,"rotation");val y=a.getJSONObject("y").toString();val z=a.getJSONObject("z").toString()
            command(session,"trim_layer_clip","object" to 2,"in_frame" to 0,"out_frame" to 100)
            val changed=command(session,"set_component","object" to 2,"property" to "rotation","axis" to "x","frame" to 5,"value" to 30)
            assertEquals(y,axes(changed,"rotation").getJSONObject("y").toString());assertEquals(z,axes(changed,"rotation").getJSONObject("z").toString())
            val properties=changed.getJSONArray("timeline_layers").getJSONObject(0).getJSONObject("properties").getJSONObject("rotation")
            assertTrue(properties.getBoolean("separated"));val x=properties.getJSONObject("axes").getJSONObject("x").getJSONArray("keys").getJSONObject(0)
            assertEquals(5,x.getInt("frame"));assertEquals(-15,x.getInt("local_frame"))
            val revision=changed.getLong("revision")
            val failed=JSONObject(NativeBridge.command(session,"{\"op\":\"move_key\",\"object\":2,\"property\":\"rotation\",\"from\":5,\"to\":10}"))
            assertFalse(failed.getBoolean("ok"));assertEquals(revision,data(NativeBridge.state(session)).getLong("revision"))
            assertFalse(JSONObject(NativeBridge.command(session,"{\"op\":\"set_component\",\"composition\":\"wrong\",\"object\":2,\"property\":\"rotation\",\"axis\":\"x\",\"frame\":5,\"value\":40}")).getBoolean("ok"))
            data(NativeBridge.save(session));File(root,"axis-response.json").writeText(changed.toString(2))
        }finally{NativeBridge.destroy(session)}
        val reopened=NativeBridge.create(root.absolutePath,"");assertTrue(reopened>0)
        try{assertTrue(axes(data(NativeBridge.state(reopened)),"rotation").has("x"))}finally{NativeBridge.destroy(reopened)}
    }
    @Test fun curvePasteAndGestureHistoryChangeOnlyTheExplicitAxis() {
        val root=root();val session=NativeBridge.create(root.absolutePath,LayerClipTimelineTest().fixture().toString());assertTrue(session>0)
        try {
            command(session,"separate_dimensions","object" to 2,"property" to "rotation")
            val original=data(NativeBridge.state(session));val originalProject=original.getJSONObject("project").toString()
            val easing=axes(original,"rotation").getJSONObject("z").getJSONArray("keys").getJSONObject(0).let{k->JSONObject().put("ease",k.getString("ease")).put("curve",JSONObject(k.getJSONObject("curve").toString()))}
            data(NativeBridge.history(session,2));command(session,"curve","object" to 2,"property" to "rotation","axis" to "y","frame" to 20,"easing" to easing)
            data(NativeBridge.history(session,4));assertEquals(originalProject,data(NativeBridge.state(session)).getJSONObject("project").toString())
            data(NativeBridge.history(session,2))
            for(i in 0..5)command(session,"curve","object" to 2,"property" to "rotation","axis" to "y","frame" to 20,
                "easing" to JSONObject().put("ease","linear").put("curve",JSONObject().put("space","progress").put("shape",JSONObject().put("kind","elastic").put("oscillations",2+i*.1).put("damping",6))))
            data(NativeBridge.history(session,3));val edited=data(NativeBridge.state(session))
            for(axis in listOf("x","z"))assertEquals(axes(original,"rotation").getJSONObject(axis).toString(),axes(edited,"rotation").getJSONObject(axis).toString())
            data(NativeBridge.history(session,0));assertEquals(originalProject,data(NativeBridge.state(session)).getJSONObject("project").toString())
            data(NativeBridge.history(session,1));assertEquals(edited.getJSONObject("project").toString(),data(NativeBridge.state(session)).getJSONObject("project").toString())
        }finally{NativeBridge.destroy(session)}
    }
    @Test fun independentlyTimedRotationAxesUseTheSameFrozenAndPngSampling() {
        val root=root();val session=NativeBridge.create(root.absolutePath,LayerClipTimelineTest().fixture().toString());assertTrue(session>0)
        try {
            command(session,"separate_dimensions","object" to 2,"property" to "rotation")
            for(axis in listOf("x","y"))command(session,"animate","object" to 2,"property" to "rotation","axis" to axis,"frame" to 20,"enabled" to false)
            command(session,"animate","object" to 2,"property" to "rotation","axis" to "x","frame" to 35,"enabled" to true)
            command(session,"set_component","object" to 2,"property" to "rotation","axis" to "x","frame" to 80,"value" to 35)
            command(session,"animate","object" to 2,"property" to "rotation","axis" to "y","frame" to 20,"enabled" to true)
            command(session,"set_component","object" to 2,"property" to "rotation","axis" to "y","frame" to 90,"value" to -25)
            val selected=listOf(19,20,35,60,80,90,99,100)
            for(f in selected){data(NativeBridge.seek(session,f.toDouble()));val file=File(data(NativeBridge.capture(session)).getString("path"));file.copyTo(File(root,"png-$f.png"),true)}
            val project=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val video=VideoExporter(root,project).run{done,_->if(done==8)command(session,"set_component","object" to 2,"property" to "rotation","axis" to "x","frame" to 80,"value" to 80)}
            val retriever=MediaMetadataRetriever();val report=JSONArray()
            try{retriever.setDataSource(video.absolutePath);assertEquals("180",retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT))
                for(f in selected){val a=BitmapFactory.decodeFile(File(root,"png-$f.png").absolutePath);val b=retriever.getFrameAtIndex(f)?:error("Frame $f did not decode")
                    try{var total=0L;var samples=0
                        for(y in 2 until 254 step 4)for(x in 2 until 254 step 4){val p=a.getPixel(x,y);val q=b.getPixel(x,y)
                            total+=kotlin.math.abs(Color.red(p)-Color.red(q))+kotlin.math.abs(Color.green(p)-Color.green(q))+kotlin.math.abs(Color.blue(p)-Color.blue(q));samples+=3}
                        val error=total.toDouble()/samples;assertTrue("Axis export mismatch $f: $error",error<6.0)
                        report.put(JSONObject().put("frame",f).put("meanRgbError",error))
                    }finally{a.recycle();b.recycle()}}
            }finally{retriever.release()}
            File(root,"axis-encoded-parity.json").writeText(JSONObject().put("comparisons",report).put("frames",180).put("frozenAgainstLiveEdits",true).toString(2))
        }finally{NativeBridge.destroy(session)}
    }
}
