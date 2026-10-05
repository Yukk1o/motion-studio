package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.*

@Composable internal fun AudioProperties(vm:EditorViewModel,modifier:Modifier) {
    val objectId=vm.selected;val audio=vm.audioClip(objectId)
    var dragging by remember{mutableStateOf(false)}
    var volume by remember(objectId){mutableFloatStateOf(audio?.optDouble("volume",1.0)?.toFloat()?:1f)}
    LaunchedEffect(objectId,audio?.optDouble("volume")){if(!dragging)volume=audio?.optDouble("volume",1.0)?.toFloat()?:1f}
    LaunchedEffect(objectId,audio?.optLong("asset")){vm.requestWaveform(objectId)}
    Column(modifier.background(Panel).padding(horizontal=12.dp).verticalScroll(rememberScrollState()).testTag("audio-properties")) {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
            if(vm.contentKind()=="video")Tool(Icons.Default.ArrowBack,"返回视频变换"){vm.property="position"}
            Text(if(vm.contentKind()=="video")"视频原声"else objectName(vm,objectId),Modifier.weight(1f),color=Ink,maxLines=1,overflow=TextOverflow.Ellipsis)
            Tool(Icons.Default.Close,"关闭声音面板"){vm.panelOpen=false}
        }
        if(audio==null){Text("此视频没有原声",color=Muted);return@Column}
        Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically){Text("音量",Modifier.weight(1f),color=Muted);Text("${(volume*100).roundToInt()}%",color=Ink)
            Switch(!audio.optBoolean("muted"),{vm.setAudio(objectId,muted=!it)},enabled=vm.editable(),modifier=Modifier.testTag("audio-enabled"))}
        StudioSlider(volume,{value->if(!dragging){vm.beginGesture();dragging=true};volume=value;vm.setAudio(objectId,volume=value.toDouble(),save=false)},valueRange=0f..2f,enabled=vm.editable(),
            onValueChangeFinished={if(dragging){vm.endGesture();dragging=false}},modifier=Modifier.heightIn(min=48.dp).testTag("audio-volume"))
        val waveform=vm.waveforms[objectId]?.optJSONArray("buckets")
        Canvas(Modifier.fillMaxWidth().height(64.dp).testTag("audio-waveform")) {
            drawLine(Muted.copy(alpha=.25f),Offset(0f,size.height/2),Offset(size.width,size.height/2),1f)
            waveform?.let{b->for(i in 0 until b.length()){
                val bucket=b.getJSONObject(i);val x=size.width*i/b.length();drawLine(if(audio.optBoolean("muted"))Muted else Accent,Offset(x,size.height*(.5f-bucket.getDouble("max").toFloat()*.45f)),Offset(x,size.height*(.5f-bucket.getDouble("min").toFloat()*.45f)),1f)
            }}
        }
        Text("长按时间轴片段移动；拖动两端裁剪。",color=Muted,fontSize=12.sp,lineHeight=18.sp)
        Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.SpaceBetween){TextButton(onClick=vm::undo,enabled=vm.state.canUndo,modifier=Modifier.height(48.dp)){Text("撤销")};TextButton(onClick=vm::splitClip,enabled=vm.editable(),modifier=Modifier.height(48.dp)){Text("当前帧分割")};TextButton(onClick=vm::duplicate,enabled=vm.editable(),modifier=Modifier.height(48.dp)){Text("复制")}}
    }
    DisposableEffect(objectId){onDispose{if(dragging)vm.cancelGesture()}}
}
