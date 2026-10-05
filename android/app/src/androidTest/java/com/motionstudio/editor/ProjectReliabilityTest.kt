package com.motionstudio.editor

import android.content.ContentValues
import android.content.Intent
import android.graphics.*
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.provider.DocumentsContract
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.StaleObjectException
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class ProjectReliabilityTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var original:File
    private val published=ArrayList<Uri>()
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private val device get()=UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
    private lateinit var originalJson:ByteArray
    private lateinit var originalAsset:ByteArray
    private val title="原工程-"+UUID.randomUUID().toString().take(8)
    private fun data(raw:String):JSONObject {
        val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")
    }
    @Before fun setup() {
        original=File(context.filesDir,"acceptance/reliability-"+UUID.randomUUID()+"/library/default").apply{mkdirs()}.canonicalFile
        val asset=File(original,"assets/original.png").apply{parentFile!!.mkdirs()}
        val bitmap=Bitmap.createBitmap(32,32,Bitmap.Config.ARGB_8888);bitmap.eraseColor(Color.argb(160,40,180,220))
        asset.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        val native=NativeBridge.create(original.absolutePath,data(NativeBridge.projectTemplate(0)).toString());assertTrue(native>0)
        try {
            val p=data(NativeBridge.state(native)).getJSONObject("project").put("name",title)
            data(NativeBridge.replace(native,p.toString()))
            val commands=JSONArray().put(JSONObject().put("op","register_asset").put("asset",JSONObject().put("id",7).put("path","assets/original.png").put("width",32).put("height",32)))
                .put(JSONObject().put("op","content").put("object",2).put("content",JSONObject().put("kind","image").put("asset",7)).put("size",JSONArray(listOf(560,760))))
            data(NativeBridge.command(native,commands.toString()));data(NativeBridge.save(native))
        } finally {NativeBridge.destroy(native)}
        originalJson=File(original,"project.json").readBytes();originalAsset=asset.readBytes()
        launch(original)
    }
    @After fun teardown() {
        if(::scenario.isInitialized)scenario.close()
        published.forEach{context.contentResolver.delete(it,null,null)}
    }
    private fun launch(root:File) {
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
    }
    private fun ready() {
        compose.waitUntil(20000){vm.state.project!=null&&!vm.state.busy&&!vm.loadFailed}
        compose.waitForIdle();assertNull(vm.state.error)
    }
    private fun photo(name:String) {
        compose.waitForIdle()
        val image=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(original.parentFile,name+".png").outputStream().use{image.compress(Bitmap.CompressFormat.PNG,100,it)};image.recycle()
    }
    private fun selectDocument(name:String) {
        fun click(selector:BySelector) {
            var last:Throwable?=null
            repeat(3) {
                device.waitForIdle(1000)
                val node=device.wait(Until.findObject(selector),5000)
                assertNotNull("System picker control is missing: "+selector,node)
                try {node!!.click();device.waitForIdle(1000);return}catch(error:StaleObjectException){last=error}
            }
            throw last?:AssertionError("System picker control did not respond")
        }
        assertTrue("System document picker was not launched",device.wait(Until.hasObject(By.pkg("com.android.documentsui")),10000))
        device.dumpWindowHierarchy(File(original.parentFile,"picker-"+name+".xml"))
        fun findDocument(timeout:Long)=device.wait(Until.findObject(By.text(name)),timeout)
            ?:device.wait(Until.findObject(By.descStartsWith(name)),timeout)
        var item=findDocument(1000)
        if(item==null) {
            val folder=if(name.endsWith(".zip"))"Download"else"Pictures"
            var folderItem=device.findObject(By.text(folder))
            if(folderItem==null) {
                val roots=device.findObject(By.descContains("根目录"))?:device.findObject(By.descContains("roots"))
                assertNotNull("Cannot open the document roots",roots)
                click(if(device.hasObject(By.descContains("根目录")))By.descContains("根目录")else By.descContains("roots"))
                device.dumpWindowHierarchy(File(original.parentFile,"picker-roots-"+name+".xml"))
                val downloads=if(name.endsWith(".zip"))device.findObject(By.text("下载"))?:device.findObject(By.text("Downloads"))else null
                if(downloads!=null) {
                    click(if(device.hasObject(By.text("下载")))By.text("下载")else By.text("Downloads"))
                    if(device.wait(Until.hasObject(By.text("MotionStudioAcceptance")),1000))click(By.text("MotionStudioAcceptance"))
                    item=findDocument(5000)
                }
                if(item==null) {
                val local=device.wait(Until.findObject(By.res("android","title").text("MuMu 共享文件")),5000)
                assertNotNull("MuMu storage root is unavailable",local);click(By.res("android","title").text("MuMu 共享文件"))
                folderItem=device.wait(Until.findObject(By.text(folder)),5000)
                }
            }
            if(item==null) {
            device.dumpWindowHierarchy(File(original.parentFile,"picker-folder-"+name+".xml"))
            assertNotNull("Storage folder is unavailable: "+folder,folderItem);click(By.text(folder))
            if(device.wait(Until.hasObject(By.text("MotionStudioAcceptance")),5000))click(By.text("MotionStudioAcceptance"))
            item=findDocument(5000)
            }
        }
        device.dumpWindowHierarchy(File(original.parentFile,"picker-selected-"+name+".xml"))
        assertNotNull("Document not found in system picker: "+name,item)
        click(if(device.hasObject(By.text(name)))By.text(name)else By.descStartsWith(name))
    }
    private fun publishImage(name:String,jpeg:Boolean=false):Uri {
        val values=ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME,name);put(MediaStore.Images.Media.MIME_TYPE,if(jpeg)"image/jpeg"else"image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH,Environment.DIRECTORY_PICTURES+"/MotionStudioAcceptance");put(MediaStore.Images.Media.IS_PENDING,1)
        }
        val uri=context.contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI,values)!!;published.add(uri)
        val image=Bitmap.createBitmap(48,64,Bitmap.Config.ARGB_8888);image.eraseColor(if(jpeg)Color.rgb(180,80,40)else Color.argb(128,255,0,0))
        context.contentResolver.openOutputStream(uri)!!.use{image.compress(if(jpeg)Bitmap.CompressFormat.JPEG else Bitmap.CompressFormat.PNG,95,it)};image.recycle()
        context.contentResolver.update(uri,ContentValues().apply{put(MediaStore.Images.Media.IS_PENDING,0)},null,null)
        return uri
    }
    private fun importImageUi(name:String,expectedAssets:Int) {
        scenario.onActivity{vm.panelOpen=false};compose.waitForIdle()
        compose.onNodeWithContentDescription("添加图层").performClick();compose.onNode(hasText("图片") and hasClickAction()).performClick()
        selectDocument(name)
        compose.waitUntil(20000){vm.state.project?.getJSONArray("assets")?.length()==expectedAssets&&!vm.state.busy}
        assertNull(vm.state.error)
    }
    @Test fun sixNewCompositionPresetsKeepThePreviousProjectAndAssets() {
        ready()
        val created=ArrayList<String>()
        for((w,h) in listOf(1080 to 1920,1920 to 1080,1080 to 1080))for(fps in listOf(30,60)) {
            compose.onNodeWithContentDescription("合成设置").performClick()
            compose.onNodeWithTag("new-"+w+"-"+h+"-"+fps).performClick()
            compose.waitUntil(15000){vm.root!=original&&!vm.state.busy&&vm.state.project?.optInt("width")==w&&vm.state.project?.optInt("height")==h&&vm.state.project?.optInt("fps")==fps}
            val p=vm.state.project!!;assertEquals(fps*6,p.getInt("frames"));assertEquals(0,p.getJSONArray("layers").length())
            assertTrue(File(vm.root,"project.json").isFile);created.add(vm.root.name)
            assertArrayEquals(originalJson,File(original,"project.json").readBytes())
            assertArrayEquals(originalAsset,File(original,"assets/original.png").readBytes())
        }
        assertEquals(6,created.toSet().size)
        compose.onNodeWithContentDescription("合成设置").performClick();compose.onNodeWithText("打开工程").performClick()
        compose.waitUntil(10000){vm.projects.any{it.name==title}}
        compose.onNodeWithText(title).performClick();compose.waitUntil(15000){vm.root==original&&!vm.state.busy}
        assertEquals(title,vm.state.project!!.getString("name"));assertEquals(1,vm.state.project!!.getJSONArray("assets").length())
        photo("project-library-restored")
        File(original.parentFile,"new-project-report.json").writeText(JSONObject().put("createdDirectories",JSONArray(created)).put("oldProjectAndAssetPreserved",true).toString(2))
    }
    @Test fun a8SystemPickerImportsPngJpgTextAndPackageRoundtripKeepsTracksAndFont() {
        ready()
        val alpha="motion-alpha-"+UUID.randomUUID().toString().take(8)+".png"
        val jpeg="motion-jpeg-"+UUID.randomUUID().toString().take(8)+".jpg"
        publishImage(alpha);importImageUi(alpha,2)
        val assets=vm.state.project!!.getJSONArray("assets")
        val cached=BitmapFactory.decodeFile(File(vm.root,assets.getJSONObject(1).getString("path")).absolutePath)
        assertEquals(128,Color.alpha(cached.getPixel(1,1)));cached.recycle()
        publishImage(jpeg,true);importImageUi(jpeg,3)
        scenario.onActivity{vm.panelOpen=false};compose.waitForIdle()
        compose.onNodeWithContentDescription("添加图层").performClick();compose.onNodeWithText("文字").performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("Motion Studio 验收")
        compose.onNodeWithText("确定").performClick()
        compose.waitUntil(15000){vm.state.project!!.getJSONArray("layers").length()==6&&!vm.state.busy}
        val textLayer=vm.layer(vm.selected)!!;assertEquals("text",textLayer.getJSONObject("content").getString("kind"))
        assertEquals("sans-bold",textLayer.getJSONObject("content").getString("font"))
        scenario.onActivity{vm.addKey();vm.seek(60.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==60.0}
        scenario.onActivity{vm.setValue(JSONArray(listOf(560,850,0)))}
        compose.waitUntil(15000){vm.keys().size==2&&!vm.state.busy}
        val before=vm.state.project!!.toString();var archive:File?=null
        scenario.onActivity{vm.output(false){archive=it}}
        compose.waitUntil(15000){archive!=null};assertTrue(archive!!.length()>0)
        val backupName="motion-backup-"+UUID.randomUUID().toString().take(8)+".zip"
        val values=ContentValues().apply {put(MediaStore.Downloads.DISPLAY_NAME,backupName);put(MediaStore.Downloads.MIME_TYPE,"application/zip")
            put(MediaStore.Downloads.RELATIVE_PATH,Environment.DIRECTORY_DOWNLOADS+"/MotionStudioAcceptance");put(MediaStore.Downloads.IS_PENDING,1)}
        val uri=context.contentResolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,values)!!;published.add(uri)
        context.contentResolver.openOutputStream(uri)!!.use{out->archive!!.inputStream().use{it.copyTo(out)}}
        context.contentResolver.update(uri,ContentValues().apply{put(MediaStore.Downloads.IS_PENDING,0)},null,null)
        val previousRoot=vm.root
        scenario.onActivity{vm.newProject(1080,1080,30)}
        compose.waitUntil(15000){vm.root!=previousRoot&&!vm.state.busy}
        compose.onNodeWithContentDescription("合成设置").performClick();compose.onNodeWithText("导入工程").performClick()
        selectDocument(backupName)
        compose.waitUntil(20000){vm.root.name.startsWith("import-")&&!vm.state.busy}
        assertEquals(before,vm.state.project!!.toString());assertTrue(vm.state.saved)
        val imported=vm.root;scenario.close();launch(imported);ready()
        assertEquals(before,vm.state.project!!.toString())
        photo("restored-assets-and-text")
        File(original.parentFile,"a8-report.json").writeText(JSONObject().put("systemPickerUsed",true).put("pngAlpha",128).put("jpgImported",true)
            .put("textFont","sans-bold").put("packageRoundtripAndReopenExact",true).toString(2))
    }
    @Test fun missingAssetShowsItsNameAndAllowsRetryWithoutChangingFiles() {
        ready();scenario.close()
        assertTrue(File(original,"assets/original.png").delete());launch(original)
        compose.waitUntil(15000){vm.loadFailed&&vm.state.error!=null}
        assertTrue(vm.state.error!!.contains("assets/original.png"))
        assertArrayEquals(originalJson,File(original,"project.json").readBytes())
        compose.onNodeWithText("知道了").performClick()
        compose.onNodeWithText("重试打开").assertIsDisplayed();compose.onNodeWithText("导入备份").assertIsDisplayed()
        photo("missing-asset-recovery")
        File(original,"assets/original.png").writeBytes(originalAsset)
        compose.onNodeWithText("重试打开").performClick();ready()
        assertFalse(vm.loadFailed);assertEquals(title,vm.state.project!!.getString("name"))
        assertArrayEquals(originalJson,File(original,"project.json").readBytes())
    }
    @Test fun corruptProjectCanStartANewProjectWhileKeepingTheDamagedFile() {
        ready();scenario.close()
        val damaged="{ invalid project data".toByteArray();File(original,"project.json").writeBytes(damaged)
        launch(original);compose.waitUntil(15000){vm.loadFailed&&vm.state.error!=null}
        compose.onNodeWithText("知道了").performClick();compose.onNodeWithText("新建工程").performClick()
        ready();assertTrue(vm.root!=original);assertFalse(vm.loadFailed)
        assertArrayEquals(damaged,File(original,"project.json").readBytes());assertArrayEquals(originalAsset,File(original,"assets/original.png").readBytes())
    }
    @Test fun openingABrokenProjectDoesNotReplaceTheActiveProject() {
        ready();scenario.onActivity{vm.newProject(1080,1080,30)}
        compose.waitUntil(15000){vm.root!=original&&!vm.state.busy};val active=vm.root;val json=vm.state.project!!.toString()
        assertTrue(File(original,"assets/original.png").delete())
        scenario.onActivity{vm.openProject("default")}
        compose.waitUntil(15000){vm.state.error!=null}
        assertTrue(vm.state.error!!.contains("assets/original.png"));assertEquals(active,vm.root);assertEquals(json,vm.state.project!!.toString())
        assertArrayEquals(originalJson,File(original,"project.json").readBytes())
    }
    @Test fun systemSaveDocumentWritesACompleteBackupThatCanBeImported() {
        fun clickStable(selector:BySelector) {
            var error:Throwable?=null
            repeat(3) {
                device.waitForIdle(1000)
                val node=device.wait(Until.findObject(selector),5000)?:throw AssertionError("Missing save control: "+selector)
                try{node.click();device.waitForIdle(1000);return}catch(stale:StaleObjectException){error=stale}
            }
            throw error?:AssertionError("Save control did not respond")
        }
        ready();val before=vm.state.project!!.toString()
        compose.onNodeWithContentDescription("输出").performClick();compose.onNodeWithText("备份工程").performClick()
        assertTrue(device.wait(Until.hasObject(By.pkg("com.android.documentsui")),10000))
        // Exercise a user-selected writable filesystem document root. MuMu's
        // separate Downloads provider has returned write-denied URIs despite
        // advertising READ|WRITE grants; its error feedback is retained below.
        val roots=device.findObject(By.descContains("根目录"))?:device.findObject(By.descContains("roots"))
        if(roots!=null) {
            clickStable(if(device.hasObject(By.descContains("根目录")))By.descContains("根目录")else By.descContains("roots"))
            device.dumpWindowHierarchy(File(original.parentFile,"save-document-roots.xml"))
            val storage=device.wait(Until.findObject(By.res("android","title").text("MuMu 共享文件")),5000)
            assertNotNull(storage);clickStable(By.res("android","title").text("MuMu 共享文件"))
            val downloads=device.wait(Until.findObject(By.text("Download")),5000)
            if(downloads!=null)clickStable(By.text("Download"))
        }
        device.dumpWindowHierarchy(File(original.parentFile,"save-document-before.xml"))
        val filename=device.wait(Until.findObject(By.res("com.android.documentsui","filename")),5000)
            ?:device.findObject(By.clazz("android.widget.EditText"))
        assertNotNull("Save document filename field is missing",filename)
        filename!!.setText("motion-saved-"+UUID.randomUUID().toString().take(8)+".motion")
        device.waitForIdle(1000)
        device.dumpWindowHierarchy(File(original.parentFile,"save-document-ready.xml"))
        val save=device.findObject(By.res("com.android.documentsui","button_save"))?:device.findObject(By.text("保存"))
            ?:device.findObject(By.text("Save"))?:device.findObject(By.res("android","button1"))
        assertNotNull("Save document action is missing",save);save!!.click()
        device.dumpWindowHierarchy(File(original.parentFile,"save-document-after-click.xml"))
        try {compose.waitUntil(20000){vm.lastSavedOutput!=null||vm.state.error!=null}}
        finally {File(original.parentFile,"save-callback-state.json").writeText(JSONObject().put("activityResult",AcceptanceActivity.lastActivityResult)
            .put("pendingFile",vm.pendingOutput?.absolutePath).put("selection",vm.lastOutputSelection?.toString()).put("saved",vm.lastSavedOutput?.toString())
            .put("error",vm.state.error).put("busy",vm.state.busy).put("outputPhase",vm.outputPhase).put("viewModelClosed",vm.isClosed).toString(2))}
        assertNull(vm.state.error);assertNotNull(vm.lastSavedOutput)
        val uri=vm.lastSavedOutput!!
        assertEquals("com.android.externalstorage.documents",uri.authority)
        val local=File(original.parentFile,"system-saved-backup.motion")
        context.contentResolver.openInputStream(uri)!!.use{input->local.outputStream().use{input.copyTo(it)}}
        assertTrue(local.length()>0)
        val previous=vm.root
        scenario.onActivity{vm.importProject(Uri.fromFile(local))}
        compose.waitUntil(15000){vm.root!=previous&&!vm.state.busy}
        assertEquals(before,vm.state.project!!.toString());assertNull(vm.state.error)
        assertTrue(DocumentsContract.deleteDocument(context.contentResolver,uri))
        File(original.parentFile,"system-save-report.json").writeText(JSONObject().put("actionCreateDocumentUsed",true).put("backupBytes",local.length()).put("roundtripExact",true).toString(2))
    }
}
