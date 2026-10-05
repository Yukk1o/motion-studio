package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaExtractor
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
class LayerClipTimelineTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun vector(x:Number,y:Number,z:Number)=JSONArray(listOf(x,y,z))
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/clips-"+UUID.randomUUID()).apply{mkdirs()}
    private fun fixture():JSONObject {
        val p=data(NativeBridge.projectTemplate(0)).put("width",256).put("height",256).put("version",2)
            .put("name","时间轴边界验收").put("background",JSONArray(listOf(0,0,1,1)))
        p.getJSONObject("camera").put("created",false).remove("parent")
        val rotation=track(vector(0,0,0)).put("keys",JSONArray()
            .put(JSONObject().put("frame",0).put("value",vector(0,0,0)).put("ease","linear")
                .put("curve",JSONObject().put("space","progress").put("shape",JSONObject().put("kind","elastic").put("oscillations",2.5).put("damping",6))))
            .put(JSONObject().put("frame",80).put("value",vector(0,0,720)).put("ease","hold")))
        val l=JSONObject().put("id",2).put("name","红色片段").put("visible",true).put("locked",false)
            .put("size",JSONArray(listOf(256,256))).put("content",JSONObject().put("kind","solid").put("color",JSONArray(listOf(1,0,0,1))))
            .put("timeline",JSONObject().put("in_frame",20).put("out_frame",100).put("offset_frame",20))
            .put("transform",JSONObject().put("position",track(vector(128,128,0))).put("rotation",rotation)
                .put("scale",track(vector(100,100,100))).put("opacity",track(1)).put("anchor",JSONArray(listOf(.5,.5))))
        return p.put("layers",JSONArray().put(l)).put("assets",JSONArray())
    }
    private fun command(session:Long,op:String,vararg fields:Pair<String,Any>):JSONObject {
        val c=JSONObject().put("op",op);fields.forEach{c.put(it.first,it.second)}
        return data(NativeBridge.command(session,c.toString()))
    }
    @Test fun commandsReturnCompositionKeysAndOneShotSplitResultsAndPersistLocalTracks() {
        val root=root();val session=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(session>0)
        try {
            val original=data(NativeBridge.state(session));val initial=original.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform").toString()
            val moved=command(session,"move_layer_clip","object" to 2,"in_frame" to 40)
            val clip=moved.getJSONArray("timeline_layers").getJSONObject(0)
            assertEquals(40,clip.getInt("offset_frame"));assertEquals(120,clip.getInt("out_frame"))
            val key=clip.getJSONObject("properties").getJSONObject("rotation").getJSONArray("keys").getJSONObject(0)
            assertEquals(40,key.getInt("frame"));assertEquals(0,key.getInt("local_frame"))
            command(session,"trim_layer_clip","object" to 2,"in_frame" to 0,"out_frame" to 120)
            command(session,"animate","object" to 2,"property" to "position","frame" to 5,"enabled" to true)
            val changed=command(session,"set_vector","object" to 2,"property" to "position","frame" to 5,"value" to vector(129,128,0))
            val negative=changed.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform").getJSONObject("position").getJSONArray("keys").getJSONObject(0)
            assertEquals(-35,negative.getInt("frame"))
            command(session,"animate","object" to 2,"property" to "position","frame" to 5,"enabled" to false)
            data(NativeBridge.replace(session,fixture().toString()))
            val split=command(session,"split_layer_clip","object" to 2,"frame" to 61)
            assertEquals(2,split.getJSONObject("edit_result").getInt("left_object"));assertEquals(3,split.getJSONObject("edit_result").getInt("right_object"))
            assertFalse(data(NativeBridge.state(session)).has("edit_result"))
            for(i in 0..1)assertEquals(initial,split.getJSONObject("project").getJSONArray("layers").getJSONObject(i).getJSONObject("transform").toString())
            data(NativeBridge.history(session,0));assertEquals(1,data(NativeBridge.state(session)).getJSONObject("project").getJSONArray("layers").length())
            data(NativeBridge.history(session,1));assertEquals(3,data(NativeBridge.state(session)).getJSONObject("project").getJSONArray("layers").getJSONObject(1).getInt("id"))
            data(NativeBridge.save(session));val saved=File(root,"project.json").readText();assertFalse(saved.contains("timeline_layers"))
            File(root,"interface-response.json").writeText(split.toString(2))
        }finally{NativeBridge.destroy(session)}
        val reopened=NativeBridge.create(root.absolutePath,"");assertTrue(reopened>0)
        try{assertEquals(2,data(NativeBridge.state(reopened)).getJSONObject("project").getJSONArray("layers").length())}finally{NativeBridge.destroy(reopened)}
    }
    @Test fun failedBatchAndFractionalInputsLeaveStateAndHistoryIntact() {
        val root=root();val session=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(session>0)
        try {
            val before=data(NativeBridge.state(session)).getJSONObject("project").toString()
            for(raw in listOf(
                "[{\"op\":\"split_layer_clip\",\"object\":2,\"frame\":61},{\"op\":\"move_layer_clip\",\"object\":3,\"in_frame\":179}]",
                "{\"op\":\"split_layer_clip\",\"object\":2,\"frame\":61.5}",
                "{\"op\":\"trim_layer_clip\",\"object\":0,\"in_frame\":0,\"out_frame\":60}")) {
                assertFalse(JSONObject(NativeBridge.command(session,raw)).getBoolean("ok"))
                val state=data(NativeBridge.state(session));assertEquals(before,state.getJSONObject("project").toString());assertEquals(0,state.getInt("revision"));assertFalse(state.getBoolean("canUndo"))
            }
        }finally{NativeBridge.destroy(session)}
    }
    @Test fun splitVisibilityPickingPngAndActualH264BoundariesAgree() {
        val root=root();val session=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(session>0)
        try {
            command(session,"split_layer_clip","object" to 2,"frame" to 61)
            val selected=listOf(0,19,20,60,61,99,100,179)
            for(f in selected) {
                val state=data(NativeBridge.seek(session,f.toDouble()))
                val active=f in 20..99
                assertEquals(if(active)1 else 0,state.getJSONArray("projectedLayers").length())
                if(active)assertEquals(if(f<61)2 else 3,state.getJSONArray("projectedLayers").getJSONObject(0).getInt("id"))
                val png=File(data(NativeBridge.capture(session)).getString("path"))
                png.copyTo(File(root,"preview-$f.png"),true)
                val bitmap=BitmapFactory.decodeFile(png.absolutePath)
                try {val pixel=bitmap.getPixel(128,128);assertTrue(if(active)Color.red(pixel)>245&&Color.blue(pixel)<10 else Color.blue(pixel)>245&&Color.red(pixel)<10)}finally{bitmap.recycle()}
            }
            data(NativeBridge.seek(session,99.5));assertEquals(1,data(NativeBridge.state(session)).getJSONArray("projectedLayers").length())
            val frozen=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val video=VideoExporter(root,frozen).run{done,_->if(done==8)command(session,"trim_layer_clip","object" to 2,"in_frame" to 20,"out_frame" to 30)}
            val extractor=MediaExtractor()
            try {extractor.setDataSource(video.absolutePath);extractor.selectTrack(0);var count=0
                while(extractor.sampleTime>=0){assertEquals((count.toLong()*1_000_000+15)/30,extractor.sampleTime);count++;if(!extractor.advance())break}
                assertEquals(180,count)
            }finally{extractor.release()}
            val retriever=MediaMetadataRetriever();val comparisons=JSONArray()
            try {retriever.setDataSource(video.absolutePath)
                for(f in selected){val bitmap=retriever.getFrameAtIndex(f)?:error("Frame $f did not decode")
                    try {val pixel=bitmap.getPixel(128,128);val active=f in 20..99
                        assertTrue("Encoded clip boundary $f",if(active)Color.red(pixel)>220&&Color.blue(pixel)<30 else Color.blue(pixel)>220&&Color.red(pixel)<30)
                        comparisons.put(JSONObject().put("frame",f).put("active",active).put("encodedCenter",pixel))
                    }finally{bitmap.recycle()}}
            }finally{retriever.release()}
            File(root,"encoded-boundaries.json").writeText(JSONObject().put("frames",180).put("boundaries",comparisons).put("frozenAgainstLiveEdits",true).toString(2))
        }finally{NativeBridge.destroy(session)}
    }
}
