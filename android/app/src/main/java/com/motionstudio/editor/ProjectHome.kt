package com.motionstudio.editor

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import kotlin.math.roundToInt

internal fun compositionFrames(seconds:String,fps:Int):Int? {
    val value=seconds.toDoubleOrNull()?:return null
    val frames=value*fps
    if(!frames.isFinite()||frames<1.0||frames>36000.0)return null
    return frames.roundToInt()
}

@Composable internal fun ProjectHome(vm:EditorViewModel,onOpen:(ProjectSummary)->Unit,onNew:(Int,Int,Int,String,Int)->Unit,onImport:()->Unit,onSettings:()->Unit,onReport:()->Unit,onUpdates:()->Unit) {
    var query by rememberSaveable{mutableStateOf("")}
    var creating by rememberSaveable{mutableStateOf(false)}
    val keyboard=LocalSoftwareKeyboardController.current
    LaunchedEffect(Unit){vm.gestureInertia.stop();vm.pause();vm.refreshProjects()}
    Surface(color=Background,modifier=Modifier.fillMaxSize().testTag("project-home")) {
        BoxWithConstraints(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.systemBars)) {
            val tablet=maxWidth>=600.dp
            val padding=if(tablet)24.dp else 16.dp
            Column(Modifier.align(Alignment.TopCenter).widthIn(max=1120.dp).fillMaxWidth().fillMaxHeight().padding(horizontal=padding)) {
                Row(Modifier.fillMaxWidth().heightIn(min=72.dp),verticalAlignment=Alignment.CenterVertically) {
                    Text("Motion Studio",Modifier.weight(1f),fontSize=22.sp,color=Ink)
                    IconButton(onClick=onSettings,modifier=Modifier.size(48.dp).testTag("home-settings")) {
                        Icon(editorIcon(Icons.Default.Settings),"设置",Modifier.size(22.dp),tint=Ink)
                    }
                }
                ReleaseUpdateBanner(vm.updates,onUpdates)
                Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(12.dp)) {
                    Button(onClick={creating=true},shape=RoundedCornerShape(12.dp),colors=ButtonDefaults.buttonColors(containerColor=Accent,contentColor=Background),modifier=Modifier.weight(1f).height(56.dp).testTag("home-new-project")) {
                        Icon(editorIcon(Icons.Default.Add),null,Modifier.size(20.dp));Spacer(Modifier.width(8.dp));Text("新建工程")
                    }
                    OutlinedButton(onClick=onImport,shape=RoundedCornerShape(12.dp),modifier=Modifier.weight(1f).height(56.dp).testTag("home-import-project")) {Text("导入工程")}
                }
                Spacer(Modifier.height(24.dp))
                Row(Modifier.fillMaxWidth().heightIn(min=40.dp),verticalAlignment=Alignment.CenterVertically) {
                    Text("最近工程",Modifier.weight(1f),fontSize=18.sp)
                    if(!vm.projectsLoading)Text("${vm.projects.size} 个工程",fontSize=12.sp,color=Muted)
                }
                BasicTextField(query,{query=it},singleLine=true,textStyle=TextStyle(color=Ink,fontSize=14.sp),cursorBrush=SolidColor(Accent),
                    keyboardOptions=KeyboardOptions(imeAction=ImeAction.Search),keyboardActions=KeyboardActions(onSearch={keyboard?.hide()}),
                    modifier=Modifier.fillMaxWidth().heightIn(min=48.dp).background(Panel,RoundedCornerShape(12.dp)).testTag("home-project-search").semantics{contentDescription="搜索工程"},
                    decorationBox={field->Row(Modifier.fillMaxWidth().heightIn(min=48.dp).padding(start=14.dp),verticalAlignment=Alignment.CenterVertically) {
                        Icon(editorIcon(Icons.Default.Search),null,Modifier.size(20.dp),tint=Muted)
                        Box(Modifier.weight(1f).padding(horizontal=10.dp)){if(query.isEmpty())Text("搜索工程",fontSize=14.sp,color=Muted);field()}
                        if(query.isNotEmpty())Tool(Icons.Default.Close,"清空工程搜索"){query="";keyboard?.hide()}
                    }})
                Spacer(Modifier.height(16.dp))
                val projects=vm.projects.filter{query.isBlank()||it.name.contains(query,true)}.sortedByDescending{it.directory==vm.root.name&&vm.state.project!=null}
                if(vm.projectsLoading&&vm.projects.isEmpty())Box(Modifier.weight(1f).fillMaxWidth(),contentAlignment=Alignment.Center){CircularProgressIndicator()}
                else if(projects.isEmpty())Column(Modifier.weight(1f).fillMaxWidth(),verticalArrangement=Arrangement.Center,horizontalAlignment=Alignment.CenterHorizontally) {
                    Text(if(query.isBlank())"开始你的第一个工程"else"没有匹配的工程",fontSize=18.sp)
                    Spacer(Modifier.height(8.dp))
                    Text(if(query.isBlank())"新建合成，或导入已有工程。"else"试试其他名称。",fontSize=14.sp,color=Muted)
                }else LazyVerticalGrid(columns=GridCells.Adaptive(280.dp),modifier=Modifier.weight(1f).fillMaxWidth().testTag("home-project-grid"),
                    horizontalArrangement=Arrangement.spacedBy(12.dp),verticalArrangement=Arrangement.spacedBy(12.dp),contentPadding=PaddingValues(bottom=24.dp)) {
                    items(projects,key={it.directory}){project->
                        val active=project.directory==vm.root.name&&vm.state.project!=null
                        Card(onClick={onOpen(project)},modifier=Modifier.fillMaxWidth().testTag("home-project-${project.directory}"),
                            colors=CardDefaults.cardColors(containerColor=Panel),border=if(active)BorderStroke(1.dp,Accent.copy(alpha=.45f))else null) {
                            Row(Modifier.fillMaxWidth().padding(14.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(14.dp)) {
                                ProjectAspectTile(project,active)
                                Column(Modifier.weight(1f)) {
                                    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                                        Text(project.name,Modifier.weight(1f),fontSize=16.sp,maxLines=2,overflow=TextOverflow.Ellipsis)
                                        if(active)Text("继续编辑",Modifier.padding(start=8.dp),fontSize=12.sp,color=Accent)
                                    }
                                    Spacer(Modifier.height(8.dp))
                                    Text("${project.width} × ${project.height} · ${project.fps} fps",fontSize=12.sp,color=Muted)
                                    Spacer(Modifier.height(4.dp))
                                    Text(SimpleDateFormat("yyyy/MM/dd HH:mm",Locale.getDefault()).format(Date(project.modified)),fontSize=12.sp,color=Muted)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if(creating)NewProjectDialog(onDismiss={creating=false}){w,h,fps,name,frames->creating=false;onNew(w,h,fps,name,frames)}
    vm.state.error?.let{message->AlertDialog(onDismissRequest=vm::clearError,title={Text("操作未完成")},text={Column{Text(message);TextButton(onClick=onReport){Text("导出错误报告")}}},confirmButton={TextButton(onClick=vm::clearError){Text("知道了")}})}
}

@Composable private fun ProjectAspectTile(project:ProjectSummary,active:Boolean) {
    Canvas(Modifier.size(64.dp).background(Background,RoundedCornerShape(8.dp))) {
        val ratio=project.width.coerceAtLeast(1).toFloat()/project.height.coerceAtLeast(1)
        val side=size.minDimension*.65f
        val w=if(ratio>1f)side else side*ratio
        val h=if(ratio>1f)side/ratio else side
        val origin=Offset((size.width-w)/2,(size.height-h)/2)
        val color=if(active)Accent else Muted
        drawRect(color.copy(alpha=.06f),origin,Size(w,h))
        drawRect(color.copy(alpha=.7f),origin,Size(w,h),style=Stroke(1.dp.toPx()))
    }
}

@Composable internal fun NewProjectDialog(onDismiss:()->Unit,title:String="新建工程",onCreate:(Int,Int,Int,String,Int)->Unit) {
    var name by remember{mutableStateOf("")}
    var width by remember{mutableStateOf("1080")};var height by remember{mutableStateOf("1920")}
    var rateText by remember{mutableStateOf("30")}
    val rate=rateText.toIntOrNull();val fps=rate?.takeIf{it in 1..240}?:30
    var duration by remember{mutableStateOf("6")}
    val frames=compositionFrames(duration,fps)
    val w=width.toIntOrNull();val h=height.toIntOrNull()
    val valid=w!=null&&h!=null&&w in 1..8192&&h in 1..8192&&rate!=null&&rate in 1..240&&frames!=null
    AlertDialog(onDismissRequest=onDismiss,containerColor=Panel,title={Text(title)},text={
        val keyboard=LocalSoftwareKeyboardController.current
        val focus=LocalFocusManager.current
        fun finishInput(){focus.clearFocus();keyboard?.hide()}
        Column(Modifier.heightIn(max=440.dp).verticalScroll(rememberScrollState())) {
        OutlinedTextField(name,{name=it.take(80)},label={Text("工程名称")},placeholder={Text("新建工程")},singleLine=true,keyboardOptions=KeyboardOptions(imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={finishInput()}),modifier=Modifier.fillMaxWidth().testTag("new-project-name"))
        Spacer(Modifier.height(16.dp))
        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(4.dp)) {
            listOf(Triple("9:16",1080,1920),Triple("16:9",1920,1080),Triple("1:1",1080,1080)).forEach{(label,x,y)->
                TextButton(onClick={finishInput();width=x.toString();height=y.toString()},modifier=Modifier.weight(1f).heightIn(min=48.dp).testTag("new-project-preset-$label")){Text(label,color=if(w==x&&h==y)Accent else Muted)}
            }
        }
        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(12.dp)) {
            OutlinedTextField(width,{width=it},label={Text("宽度")},singleLine=true,isError=w==null||w !in 1..8192,keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number,imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={finishInput()}),modifier=Modifier.weight(1f).testTag("new-project-width"))
            OutlinedTextField(height,{height=it},label={Text("高度")},singleLine=true,isError=h==null||h !in 1..8192,keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number,imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={finishInput()}),modifier=Modifier.weight(1f).testTag("new-project-height"))
        }
        if(w==null||h==null||w !in 1..8192||h !in 1..8192)Text("宽度和高度需要在 1 至 8192 之间",color=MaterialTheme.colorScheme.error,fontSize=12.sp)
        Spacer(Modifier.height(16.dp));Text("帧率",color=Muted,fontSize=13.sp)
        Row(Modifier.horizontalScroll(rememberScrollState())) {listOf(30,60,24,25,50,90,120,144,240).forEach{preset->TextButton(onClick={finishInput();rateText=preset.toString()},modifier=Modifier.heightIn(min=48.dp).testTag("new-project-fps-$preset")){Text("$preset fps",color=if(rate==preset)Accent else Muted)}}}
        OutlinedTextField(rateText,{rateText=it},label={Text("自定义帧率 · 1–240")},singleLine=true,isError=rate==null||rate !in 1..240,
            keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Number,imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={finishInput()}),modifier=Modifier.fillMaxWidth().testTag("new-project-custom-fps"))
        Spacer(Modifier.height(8.dp))
        OutlinedTextField(duration,{duration=it},label={Text("时长（秒）")},singleLine=true,isError=frames==null,
            supportingText={Text(if(frames==null)"时长需为 1 帧至 ${36000/fps} 秒"else"$frames 帧 · ${String.format(Locale.getDefault(),"%.3f",frames.toDouble()/fps)} 秒")},
            keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Decimal,imeAction=ImeAction.Done),keyboardActions=KeyboardActions(onDone={finishInput()}),
            modifier=Modifier.fillMaxWidth().testTag("new-project-duration"))
        Text("素材保存在本机。",fontSize=12.sp,color=Muted)
    }},confirmButton={TextButton(onClick={onCreate(w!!,h!!,fps,name,frames!!)},enabled=valid,modifier=Modifier.testTag("create-project")){Text("创建")}},dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}
