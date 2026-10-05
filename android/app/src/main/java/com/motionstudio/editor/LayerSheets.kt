package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

internal fun parentOf(vm:EditorViewModel,id:Long):Long? {
    val data=if(id==0L)vm.state.project?.optJSONObject("camera")else vm.layer(id)
    val link=data?.optJSONObject("parent")?:return null
    return if(link.isNull("object"))null else link.optLong("object")
}
internal fun objectName(vm:EditorViewModel,id:Long)=if(id==0L)"摄影机 1"else vm.layer(id)?.optString("name")?:"图层"
internal fun objectIcon(vm:EditorViewModel,id:Long):ImageVector=if(id==0L)Icons.Default.Videocam else when(vm.layer(id)?.optJSONObject("content")?.optString("kind")) {
    "image"->Icons.Default.Image
    "text"->Icons.Default.TextFields
    "null"->Icons.Default.ControlCamera
    else->Icons.Default.Rectangle
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun AddLayerSheet(hasCamera:Boolean,onDismiss:()->Unit,onAdd:(String)->Unit) {
    ModalBottomSheet(onDismissRequest=onDismiss,sheetState=rememberModalBottomSheetState(skipPartiallyExpanded=true),containerColor=Panel) {
        Column(Modifier.fillMaxWidth().heightIn(max=560.dp).verticalScroll(rememberScrollState()).padding(horizontal=20.dp).padding(bottom=24.dp)) {
            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text("新增图层",color=Ink,fontSize=20.sp,fontWeight=FontWeight.SemiBold)
                    Text("选择要添加的内容",color=Muted,fontSize=13.sp)
                }
                Tool(Icons.Default.Close,"关闭新增图层",action=onDismiss)
            }
            Spacer(Modifier.height(20.dp))
            Text("画面内容",color=Muted,fontSize=12.sp)
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement=Arrangement.spacedBy(10.dp)) {
                AddCard("图片","从设备导入",Icons.Default.Image,Color(0xFF83BAEB),Modifier.weight(1f).testTag("add-image")){onAdd("image")}
                AddCard("文字","标题与字幕",Icons.Default.TextFields,Color(0xFFC3A8ED),Modifier.weight(1f).testTag("add-text")){onAdd("text")}
            }
            Spacer(Modifier.height(10.dp))
            AddCard("矩形","纯色形状",Icons.Default.Rectangle,Color(0xFF83BAEB),Modifier.fillMaxWidth().testTag("add-solid")){onAdd("solid")}
            Spacer(Modifier.height(20.dp))
            Text("动画与空间",color=Muted,fontSize=12.sp)
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement=Arrangement.spacedBy(10.dp)) {
                AddCard("空对象","控制多个图层",Icons.Default.ControlCamera,Accent,Modifier.weight(1f).testTag("add-null")){onAdd("null")}
                AddCard("摄影机",if(hasCamera)"已添加"else"控制运镜",Icons.Default.Videocam,Color(0xFFE5C17E),Modifier.weight(1f).testTag("add-camera"),!hasCamera){onAdd("camera")}
            }
        }
    }
}

@Composable private fun AddCard(title:String,description:String,icon:ImageVector,color:Color,modifier:Modifier,enabled:Boolean=true,onClick:()->Unit) {
    Surface(onClick=onClick,enabled=enabled,color=Background,shape=RoundedCornerShape(12.dp),modifier=modifier.heightIn(min=88.dp)) {
        Row(Modifier.padding(14.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(10.dp)) {
            Icon(editorIcon(icon),null,tint=if(enabled)color else Muted.copy(alpha=.4f),modifier=Modifier.size(26.dp))
            Column(Modifier.weight(1f)) {
                Text(title,color=if(enabled)Ink else Muted,fontSize=15.sp,maxLines=1)
                Text(description,color=Muted,fontSize=12.sp,lineHeight=18.sp)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun ParentSheet(vm:EditorViewModel,onDismiss:()->Unit) {
    val child=vm.selected
    val objects=buildList<Long> {
        if(vm.hasCamera())add(0L)
        vm.state.project?.optJSONArray("layers")?.let{a->for(i in a.length()-1 downTo 0)add(a.getJSONObject(i).getLong("id"))}
    }
    fun allowed(id:Long):Boolean {
        var current:Long?=id
        repeat(objects.size+1) {
            if(current==child)return false
            if(current==null)return true
            current=parentOf(vm,current!!)
        }
        return false
    }
    val original=parentOf(vm,child)
    var chosen by remember(child,original){mutableStateOf(original)}
    var query by remember{mutableStateOf("")}
    val candidates=objects.filter{allowed(it)&&objectName(vm,it).contains(query.trim(),ignoreCase=true)}
    ModalBottomSheet(onDismissRequest=onDismiss,sheetState=rememberModalBottomSheetState(skipPartiallyExpanded=true),containerColor=Panel) {
        Column(Modifier.fillMaxWidth().fillMaxHeight(.78f).padding(horizontal=20.dp).padding(bottom=16.dp).testTag("parent-sheet")) {
            Row(verticalAlignment=Alignment.CenterVertically) {
                Text("父子级绑定",Modifier.weight(1f),color=Ink,fontSize=20.sp,fontWeight=FontWeight.SemiBold)
                Tool(Icons.Default.Close,"关闭父子级绑定",action=onDismiss)
            }
            Text("子级 · "+objectName(vm,child),color=Ink,fontSize=14.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
            Text("绑定后跟随父级变换，保留当前画面位置。",color=Muted,fontSize=13.sp,lineHeight=20.sp)
            Spacer(Modifier.height(12.dp))
            OutlinedTextField(query,{query=it},placeholder={Text("搜索可绑定的图层")},singleLine=true,
                leadingIcon={Icon(editorIcon(Icons.Default.Search),null)},modifier=Modifier.fillMaxWidth().testTag("parent-search"),shape=RoundedCornerShape(12.dp))
            Spacer(Modifier.height(12.dp))
            Column(Modifier.weight(1f).verticalScroll(rememberScrollState())) {
                ParentOption("不绑定父级","独立控制此图层",Icons.Default.LinkOff,chosen==null,Modifier.testTag("parent-none")){chosen=null}
                candidates.forEach{id->
                    ParentOption(objectName(vm,id),when {
                        id==0L->"摄影机"
                        vm.layer(id)?.optJSONObject("content")?.optString("kind")=="null"->"空对象 · 适合统一控制"
                        else->parentOf(vm,id)?.let{"父级 · "+objectName(vm,it)}?:"独立图层"
                    },objectIcon(vm,id),chosen==id,Modifier.testTag("parent-"+id)){chosen=id}
                }
                if(candidates.isEmpty())Text(if(query.isBlank())"暂无可绑定的父级，先新增空对象或图层。"else"没有匹配的图层",Modifier.padding(vertical=16.dp),color=Muted,fontSize=13.sp)
            }
            Spacer(Modifier.height(12.dp))
            Button(onClick={if(chosen!=original)vm.setParent(chosen);onDismiss()},enabled=vm.editable(),modifier=Modifier.fillMaxWidth().height(48.dp).testTag("parent-apply"),shape=RoundedCornerShape(12.dp)) {
                Text(if(chosen==null)if(original==null)"完成"else"解除绑定"else"绑定到 "+objectName(vm,chosen!!),maxLines=1,overflow=TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable private fun ParentOption(name:String,description:String,icon:ImageVector,selected:Boolean,modifier:Modifier,onClick:()->Unit) {
    Surface(onClick=onClick,color=if(selected)Accent.copy(alpha=.10f)else Color.Transparent,shape=RoundedCornerShape(10.dp),
        border=if(selected)BorderStroke(1.dp,Accent.copy(alpha=.45f))else null,modifier=modifier.fillMaxWidth().padding(bottom=6.dp)) {
        Row(Modifier.padding(horizontal=12.dp,vertical=12.dp).heightIn(min=40.dp),verticalAlignment=Alignment.CenterVertically) {
            Icon(editorIcon(icon),null,tint=if(selected)Accent else Muted,modifier=Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(name,color=Ink,fontSize=14.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                Text(description,color=Muted,fontSize=12.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
            }
            if(selected)Icon(editorIcon(Icons.Default.Check),"已选父级",tint=Accent,modifier=Modifier.size(20.dp))
        }
    }
}
