package com.motionstudio.editor

import android.content.Intent
import android.graphics.*
import android.os.Handler
import android.os.Looper
import android.view.PixelCopy
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.*

@RunWith(AndroidJUnit4::class)
class CameraSceneAcceptanceTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private val colors=listOf(intArrayOf(232,66,66),intArrayOf(58,206,154),intArrayOf(70,126,235))

    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/camera-"+UUID.randomUUID()).apply{mkdirs()}
        val project=fixture()
        File(root,"project.json").writeText(project.toString())
        val intent=Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath)
        scenario=ActivityScenario.launch(intent)
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        awaitPresented()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun fixture():JSONObject {
        val width=1080;val height=1920;val distance=height/(2*tan(Math.toRadians(22.5)))
        val camera=JSONObject().put("mode","position").put("position",track(JSONArray(listOf(540,960,-distance))))
            .put("target",track(JSONArray(listOf(540,960,0)))).put("roll",track(0)).put("fov",track(45))
            .put("radius",track(distance)).put("azimuth",track(0)).put("elevation",track(0))
        val layers=JSONArray()
        for(i in 0..2)layers.put(JSONObject().put("id",i+1).put("name",listOf("近层","中层","远层")[i])
            .put("visible",true).put("locked",false).put("size",JSONArray(listOf(150,300)))
            .put("content",JSONObject().put("kind","solid").put("color",JSONArray(colors[i].map{it/255.0}+1.0)))
            .put("transform",JSONObject().put("position",track(JSONArray(listOf(listOf(350,540,730)[i],960,listOf(-450,0,650)[i]))))
                .put("rotation",track(JSONArray(listOf(0,0,0)))).put("scale",track(JSONArray(listOf(100,100,100))))
                .put("opacity",track(1)).put("anchor",JSONArray(listOf(.5,.5)))))
        return JSONObject().put("version",1).put("name","摄影机验收场景").put("width",width).put("height",height)
            .put("fps",30).put("frames",180).put("background",JSONArray(listOf(.04,.05,.08,1)))
            .put("camera",camera).put("assets",JSONArray()).put("layers",layers)
    }
    private fun awaitPresented() {
        compose.waitUntil(15000) {
            scenario.onActivity{vm.refreshDiagnostics()}
            val s=vm.state.sample
            s!=null&&!s.isNull("graphics")&&!s.isNull("lastPresentedFrame")&&s.getDouble("lastPresentedFrame")==vm.frame&&
                s.getLong("lastPresentedRevision")==s.getLong("revision")&&s.getLong("lastPresentedViewRevision")==s.getLong("viewRevision")
        }
        compose.waitForIdle()
        assertNull(vm.state.error)
    }
    private fun seek(frame:Double){scenario.onActivity{vm.seek(frame)};awaitPresented()}
    private fun surface():SurfaceView {
        fun find(v:View):SurfaceView? {
            if(v is SurfaceView)return v
            if(v is ViewGroup)for(i in 0 until v.childCount)find(v.getChildAt(i))?.let{return it}
            return null
        }
        var found:SurfaceView?=null;scenario.onActivity{found=find(it.window.decorView)};return found!!
    }
    private fun copySurface():Bitmap {
        val view=surface();val bitmap=Bitmap.createBitmap(view.width,view.height,Bitmap.Config.ARGB_8888)
        val done=CountDownLatch(1);var result=-1
        scenario.onActivity{PixelCopy.request(view,bitmap,{code->result=code;done.countDown()},Handler(Looper.getMainLooper()))}
        assertTrue(done.await(10,TimeUnit.SECONDS));assertEquals(PixelCopy.SUCCESS,result)
        return bitmap
    }
    private fun save(bitmap:Bitmap,name:String) {File(root,name+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}}
    private fun capture(name:String):Bitmap {
        var file:File?=null
        scenario.onActivity{vm.output(true){generated->file=generated.copyTo(File(root,name+".png"),true)}}
        compose.waitUntil(15000){file!=null}
        return BitmapFactory.decodeFile(file!!.absolutePath)!!
    }
    private fun positions()=vm.state.project!!.getJSONArray("layers").let{a->(0 until a.length()).map{a.getJSONObject(it).getJSONObject("transform").getJSONObject("position").toString()}}
    private fun pixelBounds(bitmap:Bitmap,color:IntArray):Rect {
        var left=bitmap.width;var top=bitmap.height;var right=-1;var bottom=-1
        for(y in 0 until bitmap.height)for(x in 0 until bitmap.width) {
            val p=bitmap.getPixel(x,y)
            if(abs(Color.red(p)-color[0])+abs(Color.green(p)-color[1])+abs(Color.blue(p)-color[2])<9) {
                left=min(left,x);top=min(top,y);right=max(right,x);bottom=max(bottom,y)
            }
        }
        assertTrue("Expected colored layer was not rendered",right>=left&&bottom>=top)
        return Rect(left,top,right+1,bottom+1)
    }
    private fun exactInput(tag:String,value:String) {
        compose.onNodeWithTag(tag).performTouchInput{click()}
        compose.onNode(hasSetTextAction()).performTextReplacement(value)
        compose.onNodeWithText("确定").performClick()
        awaitPresented()
    }

    @Test fun a3DollyProducesLargerNearLayerMagnificationOnAndroidGpu() {
        val original=positions();val before=capture("a3-before")
        val widths=colors.map{pixelBounds(before,it).width().toDouble()};before.recycle()
        scenario.onActivity{vm.select(0)}
        compose.waitForIdle()
        val camera=vm.state.sample!!.getJSONObject("sampledCamera").getJSONArray("position")
        exactInput("value-Z",(camera.getDouble(2)+300).toString())
        val after=capture("a3-after")
        val ratios=colors.mapIndexed{i,c->pixelBounds(after,c).width()/widths[i]};after.recycle()
        assertTrue(ratios[0]>ratios[1]&&ratios[1]>ratios[2]&&ratios[2]>1.05)
        assertEquals(original,positions());assertEquals(45.0,vm.state.sample!!.getJSONObject("sampledCamera").getDouble("fov"),.001)
        File(root,"a3-report.json").writeText(JSONObject().put("magnificationRatios",JSONArray(ratios)).put("layerPositionsUnchanged",true).toString(2))
    }

    @Test fun a4CameraPanGestureKeepsHeadingAndHasDepthParallax() {
        val original=positions();val before=capture("a4-before")
        val centers=colors.map{pixelBounds(before,it).exactCenterX().toDouble()};before.recycle()
        scenario.onActivity{vm.select(0)};compose.waitForIdle()
        val cameraBefore=vm.state.sample!!.getJSONObject("sampledCamera")
        compose.onNodeWithTag("preview-gesture").performTouchInput{down(center);moveBy(Offset(30f,0f),150);up()}
        awaitPresented()
        val after=capture("a4-after")
        val shifts=colors.mapIndexed{i,c->abs(pixelBounds(after,c).exactCenterX()-centers[i])};after.recycle()
        val cameraAfter=vm.state.sample!!.getJSONObject("sampledCamera")
        for(i in 0..2)assertEquals(cameraBefore.getJSONArray("target").getDouble(i)-cameraBefore.getJSONArray("position").getDouble(i),
            cameraAfter.getJSONArray("target").getDouble(i)-cameraAfter.getJSONArray("position").getDouble(i),.003)
        assertEquals(0L,vm.selected);assertEquals(original,positions());assertTrue(shifts[0]>shifts[1]&&shifts[1]>shifts[2])
        File(root,"a4-report.json").writeText(JSONObject().put("pixelParallax",JSONArray(shifts)).put("headingPreserved",true).toString(2))
    }

    @Test fun a5OrbitModeCanCreateAnArcWithoutDenseHiddenKeys() {
        val original=positions()
        scenario.onActivity{vm.select(0)};compose.waitForIdle()
        compose.onNodeWithContentDescription("图层操作").performClick()
        compose.onNodeWithText("切换环绕轨道").performClick();awaitPresented()
        assertEquals(0,vm.state.project!!.getJSONObject("camera").getJSONObject("azimuth").getJSONArray("keys").length())
        compose.onNodeWithText("方位").performClick()
        compose.onNodeWithTag("property-key").performTouchInput{click()}
        compose.waitUntil(10000){vm.keys().size==1}
        val radius=vm.state.sample!!.getJSONObject("sampledCamera").getDouble("radius")
        seek(150.0);exactInput("value-方位","90")
        seek(75.0)
        val camera=vm.state.sample!!.getJSONObject("sampledCamera")
        assertEquals(45.0,camera.getDouble("azimuth"),.002)
        val eye=camera.getJSONArray("position");val target=camera.getJSONArray("target")
        assertEquals(radius,sqrt((0..2).sumOf{(eye.getDouble(it)-target.getDouble(it)).pow(2)}),.02)
        capture("a5-middle-orbit").recycle()
        assertEquals(original,positions())
        File(root,"a5-report.json").writeText(JSONObject().put("radius",radius).put("middleAzimuth",camera.getDouble("azimuth")).put("camera",camera).toString(2))
    }

    @Test fun a6ObservationOrbitZoomAndReturnDoNotModifySavedProject() {
        val project=vm.state.project!!.toString();val saved=File(root,"project.json").readBytes()
        val revision=vm.state.sample!!.getLong("revision");val reference=capture("a6-active-camera")
        scenario.onActivity{vm.view(1)};awaitPresented()
        compose.onNodeWithTag("preview-gesture").performTouchInput{down(center);moveBy(Offset(80f,25f),100);up()}
        awaitPresented()
        compose.onNodeWithTag("preview-gesture").performTouchInput {
            down(0,center+Offset(-60f,0f));down(1,center+Offset(60f,0f))
            moveTo(0,center+Offset(-90f,10f));moveTo(1,center+Offset(90f,10f));move(100)
            up(0);up(1)
        }
        awaitPresented()
        assertEquals(project,vm.state.project!!.toString());assertEquals(revision,vm.state.sample!!.getLong("revision"));assertTrue(vm.state.saved)
        val observed=capture("a6-export-while-observing")
        assertTrue(reference.sameAs(observed));reference.recycle();observed.recycle()
        assertArrayEquals(saved,File(root,"project.json").readBytes())
        scenario.onActivity{vm.view(0)};awaitPresented()
        val returned=capture("a6-returned-camera");val expected=BitmapFactory.decodeFile(File(root,"a6-active-camera.png").absolutePath)
        assertTrue(expected.sameAs(returned));expected.recycle();returned.recycle()
    }

    @Test fun a9PixelCopyPreviewMatchesTheExportedCameraFrame() {
        scenario.onActivity {
            vm.editBatch(JSONArray().put(JSONObject().put("op","animate").put("object",0).put("property","position").put("frame",0).put("enabled",true))
                .put(JSONObject().put("op","set_vector").put("object",0).put("property","position").put("frame",150)
                    .put("value",JSONArray(listOf(640,960,-1900)))))
        }
        compose.waitUntil(10000){vm.state.project!!.getJSONObject("camera").getJSONObject("position").getJSONArray("keys").length()==2}
        seek(72.0)
        val png=capture("a9-export-frame72")
        awaitPresented();val preview=copySurface();save(preview,"a9-pixelcopy-preview")
        val expected=Bitmap.createBitmap(preview.width,preview.height,Bitmap.Config.ARGB_8888)
        val canvas=Canvas(expected);canvas.drawColor(Color.rgb(10,13,20))
        val scale=min(preview.width/png.width.toFloat(),preview.height/png.height.toFloat())
        val w=png.width*scale;val h=png.height*scale
        canvas.drawBitmap(png,null,RectF((preview.width-w)/2,(preview.height-h)/2,(preview.width+w)/2,(preview.height+h)/2),Paint(Paint.FILTER_BITMAP_FLAG))
        save(expected,"a9-expected-preview")
        var difference=0L;var samples=0
        for(y in 2 until preview.height step 4)for(x in 2 until preview.width step 4) {
            val a=preview.getPixel(x,y);val b=expected.getPixel(x,y)
            difference+=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b));samples+=3
        }
        val mean=difference.toDouble()/samples
        for(color in colors) {
            val actual=pixelBounds(preview,color);val wanted=pixelBounds(expected,color)
            assertTrue(abs(actual.left-wanted.left)<=2&&abs(actual.top-wanted.top)<=2&&abs(actual.right-wanted.right)<=2&&abs(actual.bottom-wanted.bottom)<=2)
        }
        assertTrue("Preview/export RGB error="+mean,mean<3.0)
        File(root,"a9-report.json").writeText(JSONObject().put("frame",72).put("meanAbsoluteRgbError",mean).put("previewWidth",preview.width).put("previewHeight",preview.height)
            .put("graphics",vm.state.sample!!.getJSONObject("graphics")).toString(2))
        png.recycle();preview.recycle();expected.recycle()
    }

    @Test fun anchorUiPreservesProjectedGeometryAndCameraFocusKeepsEye() {
        scenario.onActivity{vm.select(2)};compose.waitForIdle()
        val before=vm.state.sample!!.getJSONArray("projectedLayers").toString()
        compose.onNodeWithContentDescription("图层操作").performClick();compose.onNodeWithText("锚点").performClick()
        compose.onNodeWithTag("anchor-x").performTextReplacement("0");compose.onNodeWithTag("anchor-y").performTextReplacement("0")
        compose.onNodeWithText("确定").performClick();awaitPresented()
        val after=vm.state.sample!!.getJSONArray("projectedLayers")
        val old=JSONArray(before)
        for(i in 0 until old.length())for(k in 0..3)for(axis in 0..1)assertEquals(old.getJSONObject(i).getJSONArray("corners").getJSONArray(k).getDouble(axis),after.getJSONObject(i).getJSONArray("corners").getJSONArray(k).getDouble(axis),.02)
        val target=vm.sampleValueFor(2,"position") as JSONArray
        val eye=vm.state.sample!!.getJSONObject("sampledCamera").getJSONArray("position").toString()
        compose.onNodeWithContentDescription("图层操作").performClick();compose.onNodeWithText("摄影机对准此图层").performClick();awaitPresented()
        assertEquals(target.toString(),vm.state.sample!!.getJSONObject("sampledCamera").getJSONArray("target").toString())
        assertEquals(eye,vm.state.sample!!.getJSONObject("sampledCamera").getJSONArray("position").toString())
    }

    @Test fun a11RealActivityStopResumeAndRecreationPreserveProjectAndFrame() {
        seek(72.0);val project=vm.state.project!!.toString();val epoch=vm.state.sample!!.getLong("surfaceEpoch")
        repeat(3) {
            scenario.moveToState(Lifecycle.State.CREATED)
            assertFalse(vm.playing)
            scenario.moveToState(Lifecycle.State.RESUMED)
            awaitPresented()
            assertEquals(project,vm.state.project!!.toString());assertEquals(72.0,vm.frame,.001)
        }
        scenario.recreate();scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]};awaitPresented()
        assertEquals(project,vm.state.project!!.toString());assertEquals(72.0,vm.frame,.001)
        assertTrue(vm.state.sample!!.getLong("surfaceEpoch")>epoch)
        assertEquals(180,vm.state.project!!.getInt("frames"));assertEquals(30,vm.state.project!!.getInt("fps"))
        File(root,"a11-report.json").writeText(JSONObject().put("stopResumeCycles",3).put("activityRecreations",1).put("frame",vm.frame).put("surfaceEpoch",vm.state.sample!!.getLong("surfaceEpoch"))
            .put("projectPreserved",true).toString(2))
    }
    @Test fun a7UiStackOrderingAndDepthOrderingMatchGpuPixels() {
        scenario.onActivity {
            val commands=JSONArray()
            for(id in 1..3)commands.put(JSONObject().put("op","set_vector").put("object",id).put("property","position").put("frame",0).put("value",JSONArray(listOf(540,960,0))))
            vm.editBatch(commands)
        }
        compose.waitUntil(10000){positions().all{JSONObject(it).getJSONArray("value").getDouble(2)==0.0}}
        awaitPresented()
        val blue=capture("a7-same-depth-before");val bluePixel=blue.getPixel(540,960);blue.recycle()
        assertTrue(abs(Color.blue(bluePixel)-235)<3)
        scenario.onActivity{vm.select(1)};compose.waitForIdle()
        repeat(2){index->
            compose.onNodeWithContentDescription("图层操作").performClick();compose.onNodeWithText("上移图层").performClick()
            compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").getJSONObject(index+1).getLong("id")==1L}
        }
        awaitPresented()
        val red=capture("a7-same-depth-reordered");val redPixel=red.getPixel(540,960);red.recycle()
        assertTrue(abs(Color.red(redPixel)-232)<3&&Color.blue(redPixel)<70)
        scenario.onActivity{vm.select(2)};compose.waitForIdle();exactInput("value-Z","-600")
        val green=capture("a7-nearer-layer");val greenPixel=green.getPixel(540,960);green.recycle()
        assertTrue(abs(Color.green(greenPixel)-206)<3&&Color.red(greenPixel)<65)
        File(root,"a7-report.json").writeText(JSONObject().put("sameDepthBeforeRgb",JSONArray(listOf(Color.red(bluePixel),Color.green(bluePixel),Color.blue(bluePixel))))
            .put("sameDepthReorderedRgb",JSONArray(listOf(Color.red(redPixel),Color.green(redPixel),Color.blue(redPixel))))
            .put("nearerLayerRgb",JSONArray(listOf(Color.red(greenPixel),Color.green(greenPixel),Color.blue(greenPixel)))).toString(2))
    }
    @Test fun orbitRecordingGestureChangesAnglesAndHasOneUndoEntry() {
        scenario.onActivity{vm.select(0);vm.cameraMode(true)}
        compose.waitUntil(10000){vm.state.project!!.getJSONObject("camera").getString("mode")=="orbit"}
        awaitPresented();val before=vm.state.project!!.toString()
        val radius=vm.state.sample!!.getJSONObject("sampledCamera").getDouble("radius")
        compose.onNodeWithTag("preview-gesture").performTouchInput{down(center);moveBy(Offset(80f,20f),150);up()}
        compose.waitUntil(10000){vm.state.canUndo&&vm.state.sample!!.getJSONObject("sampledCamera").getDouble("azimuth")>5}
        awaitPresented()
        assertEquals(0L,vm.selected);assertEquals(radius,vm.state.sample!!.getJSONObject("sampledCamera").getDouble("radius"),.001)
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.state.project!!.toString()==before}
    }
    @Test fun pausedPreviewDoesNotRedrawWhenReadingDiagnostics() {
        awaitPresented();val presented=vm.state.sample!!.getLong("presented")
        repeat(5) {
            Thread.sleep(40)
            scenario.onActivity{vm.refreshDiagnostics()}
            compose.waitForIdle()
            assertEquals(presented,vm.state.sample!!.getLong("presented"))
        }
    }
    @Test fun a1UiAnimationWorkflowSamplesAllTransformPropertiesAndEasing() {
        scenario.onActivity{vm.select(1)};compose.waitForIdle()
        val properties=listOf("position" to "位置","scale" to "缩放","rotation" to "旋转","opacity" to "透明度")
        for((key,label) in properties) {
            compose.onNodeWithText(label).performClick()
            compose.onNodeWithTag("property-key").performTouchInput{click()}
            compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").getJSONObject(0).getJSONObject("transform").getJSONObject(key).getJSONArray("keys").length()==1}
        }
        seek(60.0)
        compose.onNodeWithText("位置").performClick();exactInput("value-X","500");exactInput("value-Y","850")
        compose.onNodeWithText("缩放").performClick();exactInput("value-X","150")
        compose.onNodeWithText("旋转").performClick();exactInput("value-Z","90")
        compose.onNodeWithText("透明度").performClick();exactInput("value-透明度","40")
        compose.waitUntil(10000){properties.all{(key,_)->vm.state.project!!.getJSONArray("layers").getJSONObject(0).getJSONObject("transform").getJSONObject(key).getJSONArray("keys").length()==2}}
        seek(15.0)
        fun layerSample()=vm.state.sample!!.getJSONArray("sampledLayers").let{a->(0 until a.length()).map{a.getJSONObject(it)}.first{it.getLong("id")==1L}}
        val linear=layerSample()
        assertEquals(387.5,linear.getJSONArray("position").getDouble(0),.01)
        assertEquals(932.5,linear.getJSONArray("position").getDouble(1),.01)
        assertEquals(112.5,linear.getJSONArray("scale").getDouble(0),.01)
        assertEquals(22.5,linear.getJSONArray("rotation").getDouble(2),.01)
        assertEquals(.85,linear.getDouble("opacity"),.001)
        capture("a1-linear-frame15").recycle()
        compose.onNodeWithText("位置").performClick()
        compose.onNodeWithContentDescription("缓动曲线").performTouchInput{click()}
        compose.onNodeWithText("缓入").performTouchInput{click()}
        compose.waitUntil(10000){abs(layerSample().getJSONArray("position").getDouble(0)-359.375)<.01}
        awaitPresented();capture("a1-ease-in-frame15").recycle()
        File(root,"a1-report.json").writeText(JSONObject().put("frame",15).put("linear",linear).put("easeIn",layerSample()).put("properties",JSONArray(properties.map{it.first})).toString(2))
    }
    @Test fun a2UiKeyCopyAndCollisionMoveStayUniqueAndUndoRestoresBothKeys() {
        scenario.onActivity{vm.select(1)};compose.waitForIdle()
        compose.onNodeWithTag("property-key").performTouchInput{click()}
        compose.waitUntil(10000){vm.keys().size==1}
        seek(30.0);exactInput("value-X","450")
        compose.waitUntil(10000){vm.keys().size==2}
        val track=vm.track()!!.toString()
        val density=InstrumentationRegistry.getInstrumentation().targetContext.resources.displayMetrics.density
        fun keyDialog() {
            compose.onNodeWithTag("timeline").performTouchInput{longClick(Offset(width/2f,9*density))}
            compose.onNode(hasSetTextAction()).performTextReplacement("0")
        }
        keyDialog();compose.onNodeWithText("复制到目标帧").performClick()
        compose.waitUntil(10000){vm.keys().size==2&&vm.keys().first().getJSONArray("value").getDouble(0)==450.0}
        assertEquals(listOf(0,30),vm.keys().map{it.getInt("frame")})
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.track()!!.toString()==track}
        keyDialog();compose.onNodeWithText("精确移动").performClick()
        compose.waitUntil(10000){vm.keys().size==1&&vm.keys().first().getInt("frame")==0}
        assertEquals(450.0,vm.keys().first().getJSONArray("value").getDouble(0),.001)
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.track()!!.toString()==track}
        File(root,"a2-report.json").writeText(JSONObject().put("copiedToExistingFrame",true).put("movedOntoExistingFrame",true).put("uniqueIntegerFrames",true)
            .put("undoRestoredOriginalTrack",JSONObject(track)).toString(2))
    }
}
