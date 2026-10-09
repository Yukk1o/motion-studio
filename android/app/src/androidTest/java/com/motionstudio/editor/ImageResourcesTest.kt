package com.motionstudio.editor

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.net.Uri
import android.os.Debug
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class ImageResourcesTest {
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun root()=File(context.filesDir,"image-resource-tests/${UUID.randomUUID()}").apply{mkdirs()}
    private fun data(raw:String)=nativeData(raw)
    private fun array(vararg values:Any)=JSONArray(values.toList())
    private fun channel(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun layer(id:Int,asset:Int,start:Int=0,end:Int=12)=JSONObject().put("id",id).put("name","image $id")
        .put("visible",true).put("locked",false).put("three_d",false).put("size",array(64,64))
        .put("content",JSONObject().put("kind","image").put("asset",asset))
        .put("timeline",JSONObject().put("in_frame",start).put("out_frame",end).put("offset_frame",0))
        .put("transform",JSONObject().put("position",channel(array(32,32,0))).put("rotation",channel(array(0,0,0)))
            .put("scale",channel(array(100,100,100))).put("opacity",channel(1)).put("anchor",array(.5,.5)))
    private fun project(assets:JSONArray,layers:JSONArray):JSONObject {
        val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",12)
            .put("background",array(0,0,0,0)).put("assets",assets).put("layers",layers)
        p.getJSONObject("camera").put("created",false)
        return p
    }
    private fun asset(id:Int,path:String,w:Int,h:Int)=JSONObject().put("id",id).put("path",path).put("width",w).put("height",h)
    private fun makePng(root:File,name:String,color:Int):File {
        val file=File(root,"assets/$name").apply{parentFile!!.mkdirs()}
        val bitmap=Bitmap.createBitmap(32,16,Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(color);file.outputStream().use{assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG,100,it))};bitmap.recycle()
        return file
    }
    private fun digest(file:File):String {
        val hash=MessageDigest.getInstance("SHA-256");file.inputStream().use{input->val buffer=ByteArray(64*1024)
            while(true){val n=input.read(buffer);if(n<0)break;hash.update(buffer,0,n)}}
        return hash.digest().joinToString(""){"%02x".format(it)}
    }
    private fun bundle(native:Long,frame:Int):ByteBuffer {
        val small=ByteBuffer.allocateDirect(32).order(ByteOrder.LITTLE_ENDIAN)
        val size=-CompositionBridge.sampleFrameBundleInto(native,"comp-main",frame.toDouble(),small)
        assertTrue(size>32)
        val buffer=ByteBuffer.allocateDirect(size).order(ByteOrder.LITTLE_ENDIAN)
        assertEquals(size,CompositionBridge.sampleFrameBundleInto(native,"comp-main",frame.toDouble(),buffer));return buffer
    }
    @Test fun pngImporterPreservesOriginalAndRejectsTruncatedData() {
        val root=root();val source=makePng(root,"source.png",Color.argb(128,220,70,20));val before=digest(source)
        val imported=ImageImport.prepare(context.contentResolver,Uri.fromFile(source),root)
        assertEquals(before,digest(imported.file));assertEquals(32,imported.width);assertEquals(16,imported.height)
        val info=data(NativeBridge.prepareImage(root.absolutePath,"assets/"+imported.file.name));assertTrue(info.getBoolean("validated"))
        val broken=File(root,"assets/broken.png");val bytes=source.readBytes();broken.writeBytes(bytes.copyOf(bytes.size-12))
        assertFalse(JSONObject(NativeBridge.prepareImage(root.absolutePath,"assets/broken.png")).getBoolean("ok"))
    }
    @Test fun directBufferChecksAndPixelsPreserveLinearAlphaContract() {
        val root=root();makePng(root,"source.png",Color.argb(128,255,0,0))
        val p=project(array(asset(7,"assets/source.png",32,16)),array(layer(1,7)))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(native!=0L)
        try {
            for(buffer in listOf(ByteBuffer.allocate(2048),ByteBuffer.allocateDirect(2047),ByteBuffer.allocateDirect(2048).asReadOnlyBuffer()))
                assertFalse(JSONObject(NativeBridge.assetPixelsInto(native,7,buffer)).getBoolean("ok"))
            val pixels=ByteBuffer.allocateDirect(2048);val info=data(NativeBridge.assetPixelsInto(native,7,pixels))
            assertEquals("original",info.getString("resolution"));assertEquals(2048,info.getInt("bytes"))
            assertTrue((pixels.get(0).toInt() and 255) in 187..189);assertEquals(128,pixels.get(3).toInt() and 255)
        }finally{NativeBridge.destroy(native)}
    }
    @Test fun glesLoadsSharedImagesOnlyWhenActiveAndMatchesNativeCapture() {
        val root=root();makePng(root,"red.png",Color.argb(128,255,0,0));makePng(root,"blue.png",Color.BLUE);makePng(root,"unused.png",Color.GREEN)
        val p=project(array(asset(99,"assets/unused.png",32,16),asset(7,"assets/red.png",32,16),asset(8,"assets/blue.png",32,16)),
            array(layer(1,7,0,6),layer(2,7,0,6),layer(3,8,6,12)))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(native!=0L)
        try {
            // Native headless capture may bind an EGL context on this thread.
            // Complete native captures before creating the export context.
            val samples=listOf(0,2,8,2).map { frame ->
                val plan=bundle(native,frame)
                val path=data(CompositionBridge.capture(native,"comp-main")).getString("path")
                val png=BitmapFactory.decodeFile(path,BitmapFactory.Options().apply{inPremultiplied=false;inScaled=false})
                Triple(frame,plan,png)
            }
            try {
                val gpu=EglMovieRenderer(null,64,64,p,native,data(NativeBridge.renderPlanInfo(native)))
                try {
                    assertEquals(4L,gpu.residentImageBytes);assertEquals(0L,gpu.imageUploads)
                    val reports=JSONArray()
                    for((frame,plan,png) in samples) {
                        gpu.prepareBundle(plan);gpu.drawBundle(plan)
                        assertEquals(2052L,gpu.residentImageBytes)
                        val pixels=ByteBuffer.allocateDirect(64*64*4);gpu.readPixelsInto(pixels)
                        reports.put(glesParity(png,pixels).put("frame",frame))
                    }
                    assertEquals(3L,gpu.imageUploads)
                    File(root,"unencoded-report.json").writeText(reports.toString(2))
                }finally{gpu.close()}
            }finally{samples.forEach{it.third.recycle()}}
        }finally{NativeBridge.destroy(native)}
    }
    @Test fun optionalLargeOriginalPng() {
        val path=InstrumentationRegistry.getArguments().getString("largeImagePath")
        assumeTrue("Pass a local PNG through largeImagePath",!path.isNullOrBlank())
        val source=File(path!!);assertTrue(source.isFile);val root=root();val before=digest(source)
        val started=System.nanoTime();val imported=ImageImport.prepare(context.contentResolver,Uri.fromFile(source),root)
        val elapsed=(System.nanoTime()-started)/1_000_000
        assertEquals(7952,imported.width);assertEquals(3273,imported.height);assertEquals(before,digest(imported.file))
        val info=data(NativeBridge.prepareImage(root.absolutePath,"assets/"+imported.file.name))
        assertEquals(2048,info.getInt("proxyWidth"));assertEquals(843,info.getInt("proxyHeight"));assertTrue(info.getBoolean("proxyCached"))
        val unused=File(root,"assets/unused.png");imported.file.copyTo(unused)
        val p=project(array(asset(99,"assets/unused.png",7952,3273),asset(7,"assets/"+imported.file.name,7952,3273)),array(layer(1,7),layer(2,7)))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(native!=0L)
        try {
            val gpu=EglMovieRenderer(null,64,64,p,native,data(NativeBridge.renderPlanInfo(native)))
            try {
                assertEquals(4L,gpu.residentImageBytes)
                val plan=bundle(native,0);val upload=System.nanoTime();gpu.prepareBundle(plan);gpu.drawBundle(plan)
                val uploadMs=(System.nanoTime()-upload)/1_000_000
                assertEquals(1L,gpu.imageUploads);assertEquals(4L+7952L*3273*4,gpu.residentImageBytes)
                gpu.prepareBundle(plan);assertEquals(1L,gpu.imageUploads)
                val output=ByteBuffer.allocateDirect(64*64*4);gpu.readPixelsInto(output)
                assertTrue("Large original rendered an empty frame",(0 until 64*64).count{(output.get(it*4+3).toInt() and 255)>0}>50)
                val report=JSONObject().put("width",imported.width).put("height",imported.height).put("sourceBytes",imported.file.length())
                    .put("sourceSha256",before).put("importMs",elapsed).put("proxyWidth",2048).put("proxyHeight",843)
                    .put("originalUploadMs",uploadMs).put("residentImageBytes",gpu.residentImageBytes).put("imageUploads",gpu.imageUploads)
                    .put("pssKiBAfterUpload",Debug.getPss()).put("nativeHeapBytesAfterUpload",Debug.getNativeHeapAllocatedSize())
                File(context.filesDir,"large-image-report.json").writeText(report.toString(2))
            }finally{gpu.close()}
        }finally{NativeBridge.destroy(native);imported.file.delete();unused.delete()}
    }
}
