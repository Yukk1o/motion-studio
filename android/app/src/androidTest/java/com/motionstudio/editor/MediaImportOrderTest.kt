package com.motionstudio.editor

import android.app.Application
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.After
import org.junit.Before
import org.junit.Test
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

/** Exercise the real import commands without requiring a preview surface. */
@RunWith(AndroidJUnit4::class)
class MediaImportOrderTest {
    private val instrumentation get()=InstrumentationRegistry.getInstrumentation()
    private val app get()=instrumentation.targetContext.applicationContext as Application
    private lateinit var root:File
    private lateinit var image:File
    private lateinit var vm:EditorViewModel
    private var store=ViewModelStore()

    private fun <T> onMain(action:()->T):T {
        var result:Result<T>?=null
        instrumentation.runOnMainSync {result=runCatching(action)}
        return result!!.getOrThrow()
    }
    private fun await(message:String,ready:()->Boolean) {
        val deadline=System.nanoTime()+45_000_000_000L
        while(!onMain {
            assertNull("$message: ${vm.state.error}",vm.state.error)
            ready()
        }) {
            assertTrue(message,System.nanoTime()<deadline)
            Thread.sleep(20)
        }
    }
    private fun open(initial:String="") {
        onMain {vm=EditorViewModel(app,root,initial);store.put("editor",vm)}
        await("Open project"){vm.state.project!=null&&vm.importTask==null&&!vm.state.busy}
    }
    @Before fun setup() {
        root=File(app.filesDir,"acceptance/import-order-${UUID.randomUUID()}").apply{mkdirs()}
        image=File(root,"input.png")
        Bitmap.createBitmap(32,24,Bitmap.Config.ARGB_8888).apply {
            eraseColor(Color.argb(128,255,32,16))
            image.outputStream().use {assertTrue(compress(Bitmap.CompressFormat.PNG,100,it))}
            recycle()
        }
        val project=nativeData(NativeBridge.projectTemplate(0))
            .put("width",256).put("height",144).put("fps",60).put("frames",240)
            .put("assets",JSONArray()).put("audio_assets",JSONArray())
            .put("video_assets",JSONArray()).put("layers",JSONArray())
        open(project.toString())
    }
    @After fun teardown() {onMain{store.clear()}}

    private fun project()=onMain{JSONObject(vm.state.project!!.toString())}
    private fun assertReferences() {
        val p=project()
        val ids=listOf("assets","audio_assets","video_assets").flatMap {key->
            p.optJSONArray(key).objects().map{it.getLong("id")}
        }
        assertTrue(ids.all{it>0})
        assertEquals("Asset IDs must be unique across every media table",ids.size,ids.toSet().size)
        val rasterIds=p.getJSONArray("assets").objects().map{it.getLong("id")}.toSet()
        p.getJSONArray("layers").objects().forEach {layer->
            val content=layer.getJSONObject("content")
            when(content.getString("kind")) {
                "image"->assertTrue(rasterIds.contains(content.getLong("asset")))
                "text"->assertTrue(rasterIds.contains(content.getLong("raster_asset")))
            }
        }
    }
    private fun addRaster(text:Boolean):JSONObject {
        val before=project()
        val count=before.getJSONArray("assets").length()
        onMain {if(text)vm.addText("Motion Studio")else vm.importImage(Uri.fromFile(image))}
        await(if(text)"Add text"else"Import image") {
            vm.state.project!!.getJSONArray("assets").length()==count+1&&!vm.state.busy&&vm.state.saved
        }
        val after=project()
        assertEquals(before.getJSONArray("layers").length()+1,after.getJSONArray("layers").length())
        for(key in listOf("audio_assets","video_assets")) {
            assertEquals("Raster import changed $key",before.optJSONArray(key)?.toString(),after.optJSONArray(key)?.toString())
        }
        val content=onMain{vm.layer(vm.selected)!!.getJSONObject("content")}
        assertEquals(if(text)"text"else"image",content.getString("kind"))
        assertReferences()
        return before
    }
    private fun importMedia(name:String,kind:String="video",sound:Boolean=true) {
        val count=project().getJSONArray("layers").length()
        onMain {vm.importMedia(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/$name"),kind,sound)}
        await("Import $name") {
            vm.importTask==null&&vm.state.project!!.getJSONArray("layers").length()==count+1&&vm.state.saved
        }
        assertReferences()
    }

    @Test fun silentVideoThenImageAndTextCanUndoRedoAndReopen() {
        importMedia("silent-24fps.mp4",sound=false)
        addRaster(false)
        val beforeText=addRaster(true)
        val afterText=project()
        onMain {vm.undo()}
        await("Undo text"){vm.state.project!!.toString()==beforeText.toString()&&vm.state.saved}
        onMain {vm.redo()}
        await("Redo text"){vm.state.project!!.toString()==afterText.toString()&&vm.state.saved}
        onMain {store.clear();store=ViewModelStore()}
        open()
        assertEquals(afterText.toString(),project().toString())
        addRaster(false)
        addRaster(true)
    }
    @Test fun videoWithOriginalSoundThenTextAndImageHaveDistinctAssets() {
        importMedia("sound-24fps.mp4")
        assertEquals(1,project().getJSONArray("audio_assets").length())
        addRaster(true)
        addRaster(false)
    }
    @Test fun audioThenImageAndTextAlsoUseTheSharedAssetNamespace() {
        importMedia("tone-stereo-48000.wav",kind="audio")
        addRaster(false)
        addRaster(true)
    }
    @Test fun imagesAndTextBeforeAndAfterVideoKeepValidReferences() {
        addRaster(false)
        addRaster(true)
        importMedia("sound-24fps.mp4")
        addRaster(false)
        addRaster(true)
    }
}
