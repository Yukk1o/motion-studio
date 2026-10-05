package com.motionstudio.editor

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.*
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.ShowChart
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import java.util.Locale
import kotlin.math.*

@Composable internal fun EditorFooter(vm:EditorViewModel) {
    var more by remember{mutableStateOf(false)}
    Row(Modifier.fillMaxWidth().height(52.dp).background(Panel).padding(horizontal=8.dp)
        .then(if(vm.panelOpen)Modifier.clearAndSetSemantics{}else Modifier),verticalAlignment=Alignment.CenterVertically) {
        Text(if(vm.selected==0L){if(vm.hasCamera())"摄影机 1"else"合成视图"} else vm.layer(vm.selected)?.optString("name")?:"图层",
            Modifier.weight(1f),color=Ink,fontSize=12.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
        listOf("position" to "移动","rotation" to "旋转","scale" to "缩放").forEach{(key,label)->
            if(vm.selected!=0L||(vm.hasCamera()&&key=="position"))TextButton(onClick={vm.openProperty(key)},modifier=Modifier.height(48.dp)) {
                Text(label,color=Accent,fontSize=12.sp)
            }
        }
        if(vm.selected==0L&&vm.hasCamera())TextButton(onClick={vm.openProperty("fov")},modifier=Modifier.height(48.dp)){Text("镜头",color=Accent,fontSize=12.sp)}
        Box {
            Tool(Icons.Default.MoreHoriz,"图层快捷操作"){more=true}
            DropdownMenu(more,{more=false}) {
                if(vm.selected!=0L) {
                    DropdownMenuItem(text={Text("透明度")},onClick={more=false;vm.openProperty("opacity")})
                    DropdownMenuItem(text={Text("复制图层")},onClick={more=false;vm.duplicate()})
                    DropdownMenuItem(text={Text("删除图层")},onClick={more=false;vm.deleteLayer()})
                } else if(vm.hasCamera())DropdownMenuItem(text={Text("目标点")},onClick={more=false;vm.openProperty("target")})
            }
        }
    }
}

private fun stepFor(key:String)=when(key){"opacity"->.005;"rotation","fov","roll","azimuth","elevation"->.25;"scale"->.5;else->1.0}
private fun bounded(key:String,value:Double)=when(key){
    "opacity"->value.coerceIn(0.0,1.0);"fov"->value.coerceIn(10.0,120.0)
    "radius"->value.coerceIn(1.0,10_000_000.0);"elevation"->value.coerceIn(-89.0,89.0)
    "scale"->value.coerceIn(-100_000.0,100_000.0)
    "rotation","roll","azimuth"->value.coerceIn(-1_000_000.0,1_000_000.0)
    else->value.coerceIn(-10_000_000.0,10_000_000.0)
}

/** A drag captures its target and starting value once. Delayed JNI replies cannot
 * change the drag's base, current property, or the other vector components. */
private class ValueDrag(private val vm:EditorViewModel) {
    private val objectId=vm.selected
    private val key=vm.property
    private val frame=floor(vm.frame).toInt()
    private val original=vm.sampleValue().let{if(it is JSONArray)JSONArray(it.toString()) else it}
    private val offsets=DoubleArray(3)
    fun adjust(axis:Int,amount:Double) {
        offsets[axis]+=amount*stepFor(key)
        publish(axis)
    }
    fun adjustXY(x:Double,y:Double) {
        offsets[0]+=x*stepFor(key);offsets[1]+=y*stepFor(key)
        publish(0)
    }
    private fun publish(axis:Int) {
        if(original is JSONArray) {
            val value=JSONArray(original.toString())
            for(i in 0..2)value.put(i,bounded(key,original.getDouble(i)+offsets[i]))
            if(key=="scale"&&vm.scaleLinked&&axis<2) {
                val other=1-axis;val base=original.getDouble(axis)
                value.put(other,bounded(key,if(abs(base)>.0001)original.getDouble(other)*value.getDouble(axis)/base else value.getDouble(axis)))
            }
            vm.setPropertyValue(objectId,key,frame,value,false)
        } else if(original is Number)vm.setPropertyValue(objectId,key,frame,bounded(key,original.toDouble()+offsets[0]),false)
    }
}

@Composable internal fun Properties(vm:EditorViewModel,modifier:Modifier) {
    var rename by remember{mutableStateOf(false)}
    var anchor by remember{mutableStateOf(false)}
    var more by remember{mutableStateOf(false)}
    var parenting by remember{mutableStateOf(false)}
    var curves by remember(vm.selected,vm.property){mutableStateOf(false)}
    BackHandler(enabled=curves){curves=false}
    val camera=vm.selected==0L
    val mode=vm.state.project?.optJSONObject("camera")?.optString("mode")?:"position"
    val choices=if(camera) {
        if(mode=="orbit")listOf("radius" to "距离","azimuth" to "方位","elevation" to "俯仰","target" to "目标点","fov" to "视角","roll" to "滚转")
        else listOf("position" to "位置","target" to "目标点","fov" to "视角","roll" to "滚转")
    } else listOf("position" to "位置","rotation" to "旋转","scale" to "缩放","opacity" to "透明度")
    Column(modifier.testTag("properties-panel").background(Panel,RoundedCornerShape(topStart=12.dp,topEnd=12.dp))
        .pointerInput(Unit){awaitPointerEventScope{while(true)awaitPointerEvent()}}.padding(horizontal=8.dp)) {
        Row(Modifier.fillMaxWidth().height(48.dp),verticalAlignment=Alignment.CenterVertically) {
            if(curves)Tool(Icons.AutoMirrored.Filled.ArrowBack,"返回变换参数"){curves=false}
            Text(if(curves)"缓动曲线" else if(camera)"摄影机 1" else vm.layer(vm.selected)?.optString("name")?:"图层",
                Modifier.weight(1f),color=Ink,fontSize=14.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
            if(!curves)Tool(Icons.AutoMirrored.Filled.ShowChart,"缓动曲线",vm.easingSegment()!=null){curves=true}
            Box {
                Tool(Icons.Default.MoreVert,"图层操作"){more=true}
                DropdownMenu(more,{more=false}) {
                    DropdownMenuItem(text={Text("父级")},enabled=vm.editable(),onClick={more=false;parenting=true})
                    if(camera) {
                        DropdownMenuItem(text={Text(if(mode=="orbit")"切换位置路径" else "切换环绕轨道")},onClick={more=false;vm.cameraMode(mode!="orbit");vm.property=if(mode=="orbit")"position" else "radius"})
                        DropdownMenuItem(text={Text("删除摄影机")},onClick={more=false;vm.deleteLayer()})
                    }
                    else {
                        DropdownMenuItem(text={Text("锚点")},enabled=vm.editable(),onClick={more=false;anchor=true})
                        DropdownMenuItem(text={Text("摄影机对准此图层")},onClick={more=false;vm.focusCameraOnSelection()})
                        DropdownMenuItem(text={Text("重命名")},onClick={more=false;rename=true})
                        DropdownMenuItem(text={Text("复制图层")},onClick={more=false;vm.duplicate()})
                        DropdownMenuItem(text={Text("上移图层")},onClick={more=false;vm.reorder(1)})
                        DropdownMenuItem(text={Text("下移图层")},onClick={more=false;vm.reorder(-1)})
                        DropdownMenuItem(text={Text("锁定 / 解锁")},onClick={more=false;vm.layer(vm.selected)?.let{vm.flags(vm.selected,it.getBoolean("visible"),!it.getBoolean("locked"))}})
                        DropdownMenuItem(text={Text("删除图层")},onClick={more=false;vm.deleteLayer()})
                    }
                    if(vm.keys().isNotEmpty())DropdownMenuItem(text={Text("移除此属性动画")},enabled=vm.editable(),onClick={more=false;vm.animate()})
                }
            }
            Tool(Icons.Default.Close,"关闭属性面板"){vm.panelOpen=false}
        }
        Row(Modifier.weight(1f).fillMaxWidth()) {
            Column(Modifier.width(48.dp),horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.SpaceEvenly) {
                Tool(Icons.Default.SkipPrevious,"上一关键帧",vm.keys().any{it.getInt("frame")<vm.frame}){vm.jumpKey(false)}
                Box(Modifier.testTag("property-key")) {
                    Tool(Icons.Default.Diamond,if(vm.currentKey()==null)"添加关键帧" else "删除当前关键帧",vm.editable(),vm::toggleKey)
                    Text(if(vm.currentKey()==null)"+" else "−",Modifier.align(Alignment.Center),color=if(vm.currentKey()!=null)Background else Ink,fontSize=13.sp)
                }
                Tool(Icons.Default.SkipNext,"下一关键帧",vm.keys().any{it.getInt("frame")>vm.frame}){vm.jumpKey(true)}
            }
            if(curves)CurveEditor(vm,Modifier.weight(1f).fillMaxHeight())
            else Column(Modifier.weight(1f).fillMaxHeight().testTag("property-values")) {
                Row(Modifier.fillMaxWidth().height(48.dp).horizontalScroll(rememberScrollState())) {
                    choices.forEach{(key,label)->TextButton(onClick={vm.pause();vm.property=key},modifier=Modifier.height(48.dp)
                        .testTag("property-tab-"+key).semantics{selected=vm.property==key;role=Role.Tab}) {
                        Column(horizontalAlignment=Alignment.CenterHorizontally) {
                            Text(label,fontSize=12.sp,lineHeight=16.sp,maxLines=1,color=if(vm.property==key)Accent else Muted)
                            Spacer(Modifier.height(4.dp))
                            Box(Modifier.height(2.dp).width(24.dp).background(if(vm.property==key)Accent else Color.Transparent))
                        }
                    }}
                }
                val value=vm.sampleValue()
                if(value is JSONArray)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(4.dp)) {
                    for(i in 0 until min(3,value.length()))ScrubField(vm,listOf("X","Y","Z")[i],value.getDouble(i),i,Modifier.weight(1f))
                } else if(value is Number)ScrubField(vm,choices.firstOrNull{it.first==vm.property}?.second?:"数值",value.toDouble(),0,Modifier.fillMaxWidth())
                TransformPad(vm,Modifier.weight(1f).fillMaxWidth().padding(vertical=4.dp))
            }
        }
    }
    if(rename)InputDialog("图层名称",vm.layer(vm.selected)?.optString("name")?:"",onDismiss={rename=false}){vm.rename(it);rename=false}
    if(anchor)AnchorDialog(vm){anchor=false}
    if(parenting)ParentDialog(vm){parenting=false}
}

@Composable private fun ParentDialog(vm:EditorViewModel,onDismiss:()->Unit) {
    val objects=buildList<Pair<Long,String>> {
        if(vm.hasCamera())add(0L to "摄影机 1")
        vm.state.project?.optJSONArray("layers")?.let{array->for(i in 0 until array.length()){val l=array.getJSONObject(i);add(l.getLong("id") to l.getString("name"))}}
    }
    fun parentOf(id:Long):Long? {
        val objectData=if(id==0L)vm.state.project?.optJSONObject("camera")else vm.layer(id)
        val link=objectData?.optJSONObject("parent")?:return null
        return if(link.isNull("object"))null else link.optLong("object")
    }
    fun allowed(id:Long):Boolean {
        var current:Long?=id
        repeat(objects.size+1){if(current==vm.selected)return false;if(current==null)return true;current=parentOf(current!!)}
        return false
    }
    val current=parentOf(vm.selected)
    AlertDialog(onDismissRequest=onDismiss,title={Text("设置父级")},text={Column(Modifier.heightIn(max=360.dp).verticalScroll(rememberScrollState())) {
        Text("保持当前画面；子级继承父级的位置、旋转和缩放",fontSize=12.sp,lineHeight=18.sp,color=Muted)
        TextButton(onClick={vm.setParent(null);onDismiss()},modifier=Modifier.fillMaxWidth().heightIn(min=48.dp).testTag("parent-none")){Text("解除绑定")}
        objects.filter{allowed(it.first)}.forEach{(id,name)->TextButton(onClick={vm.setParent(id);onDismiss()},modifier=Modifier.fillMaxWidth().heightIn(min=48.dp).testTag("parent-"+id)) {
            Text(name,color=if(current==id)Accent else Ink)
        }}
    }},confirmButton={TextButton(onClick=onDismiss){Text("关闭")}})
}

@Composable private fun AnchorDialog(vm:EditorViewModel,onDismiss:()->Unit) {
    val value=vm.layer(vm.selected)?.getJSONObject("transform")?.getJSONArray("anchor")?:return
    var x by remember{mutableStateOf(String.format(Locale.US,"%.2f",value.getDouble(0)*100))}
    var y by remember{mutableStateOf(String.format(Locale.US,"%.2f",value.getDouble(1)*100))}
    val nx=x.toDoubleOrNull();val ny=y.toDoubleOrNull()
    AlertDialog(onDismissRequest=onDismiss,title={Text("锚点")},text={Column {
        Text("修改后保持当前画面位置",color=Muted,fontSize=12.sp)
        OutlinedTextField(x,{x=it},label={Text("X %")},modifier=Modifier.testTag("anchor-x"),singleLine=true)
        OutlinedTextField(y,{y=it},label={Text("Y %")},modifier=Modifier.testTag("anchor-y"),singleLine=true)
        TextButton(onClick={x="50";y="50"}){Text("居中")}
    }},confirmButton={TextButton(enabled=nx!=null&&ny!=null&&nx.isFinite()&&ny.isFinite()&&abs(nx)<=100000&&abs(ny)<=100000,
        onClick={vm.anchor(nx!!/100,ny!!/100);onDismiss()}){Text("确定")}},dismissButton={TextButton(onClick=onDismiss){Text("取消")}})
}

@Composable private fun ScrubField(vm:EditorViewModel,label:String,value:Double,axis:Int,modifier:Modifier) {
    var editing by remember(vm.selected,vm.property){mutableStateOf(false)}
    var drag by remember{mutableStateOf<ValueDrag?>(null)}
    val density=LocalDensity.current.density
    val factor=if(vm.property=="opacity")100.0 else 1.0
    val key=vm.property;val objectId=vm.selected;val frame=floor(vm.frame).toInt()
    Surface(color=Background,shape=RoundedCornerShape(4.dp),modifier=modifier.heightIn(min=56.dp).testTag("value-"+label)
        .draggable(rememberDraggableState{delta->drag?.adjust(axis,delta/density.toDouble())},Orientation.Horizontal,
            enabled=vm.editable(),onDragStarted={drag=ValueDrag(vm);vm.beginGesture()},onDragStopped={vm.endGesture();drag=null})
        .clickable(enabled=vm.editable()){vm.pause();editing=true}) {
        Column(Modifier.padding(horizontal=6.dp,vertical=6.dp),verticalArrangement=Arrangement.Center) {
            Text(label+(if(key=="scale"||key=="opacity")" %" else if(key=="rotation"||key=="fov"||key=="roll")" °" else ""),color=Muted,fontSize=11.sp,lineHeight=14.sp,maxLines=1)
            Text(String.format(Locale.US,"%.1f",value*factor),modifier=Modifier.testTag("number-"+label),color=if(vm.editable())Ink else Muted,
                fontSize=14.sp,lineHeight=18.sp,fontFamily=FontFamily.Monospace,maxLines=1,overflow=TextOverflow.Ellipsis)
        }
    }
    if(editing)InputDialog(label,String.format(Locale.US,"%.3f",value*factor),onDismiss={editing=false},numeric=true){input->
        val number=input.replace(',','.').toDoubleOrNull()
        if(number!=null&&number.isFinite()) {
            val original=vm.sampleValue()
            val raw=bounded(key,number/factor)
            val next=if(original is JSONArray)JSONArray(original.toString()).put(axis,raw).also{array->
                if(key=="scale"&&vm.scaleLinked&&axis<2) {
                    val base=original.getDouble(axis);val other=1-axis
                    array.put(other,bounded(key,if(abs(base)>.0001)original.getDouble(other)*raw/base else raw))
                }
            } else raw
            vm.setPropertyValue(objectId,key,frame,next);editing=false
        }
    }
}

@Composable private fun TransformPad(vm:EditorViewModel,modifier:Modifier) {
    var drag:ValueDrag? by remember{mutableStateOf(null)}
    var zAxis by remember(vm.property){mutableStateOf(false)}
    val density=LocalDensity.current.density
    val vector=vm.sampleValue() is JSONArray
    Box(modifier.background(Background,RoundedCornerShape(4.dp)).testTag("transform-pad")
        .pointerInput(vm.selected,vm.property,zAxis,vm.scaleLinked,vm.editable()) {
            if(vm.editable())detectDragGestures(onDragStart={drag=ValueDrag(vm);vm.beginGesture()},
                onDragEnd={vm.endGesture();drag=null},onDragCancel={vm.endGesture();drag=null}){change,amount->
                change.consume()
                if(vector&&vm.property in listOf("position","target")) {
                    if(zAxis)drag?.adjust(2,-amount.y/density.toDouble())
                    else drag?.adjustXY(amount.x/density.toDouble(),amount.y/density.toDouble())
                } else drag?.adjust(if(vm.property=="rotation")2 else 0,(amount.x-amount.y)/density.toDouble())
            }
        }) {
        Canvas(Modifier.fillMaxSize()) {
            drawLine(Muted.copy(alpha=.10f),Offset(size.width/2,0f),Offset(size.width/2,size.height),1f)
            drawLine(Muted.copy(alpha=.10f),Offset(0f,size.height/2),Offset(size.width,size.height/2),1f)
            for(i in 1..7)drawLine(Muted.copy(alpha=.1f),Offset(size.width*i/8,0f),Offset(size.width*i/8,4.dp.toPx()),1f)
        }
        Column(Modifier.align(Alignment.Center),horizontalAlignment=Alignment.CenterHorizontally) {
        Text(if(!vm.editable())"图层已锁定" else when(vm.property) {
            "position","target"->if(zAxis)"上下滑动调整 Z" else "滑动移动 · XY"
            "rotation"->"滑动旋转 · Z";"scale"->"滑动缩放";"opacity"->"滑动调整透明度";else->"滑动调节数值"
        },color=Muted,fontSize=12.sp,lineHeight=18.sp)
        if(vm.editable())Text("数值可滑动 · 点按输入",color=Muted.copy(alpha=.7f),fontSize=10.sp,lineHeight=14.sp)
        }
        if(vm.property in listOf("position","target"))TextButton(onClick={zAxis=!zAxis},modifier=Modifier.align(Alignment.TopEnd).size(48.dp)) {
            Text(if(zAxis)"XY" else "Z",color=Accent,fontSize=11.sp,lineHeight=14.sp)
        }
        if(vm.property=="scale")IconButton(onClick={vm.scaleLinked=!vm.scaleLinked},modifier=Modifier.align(Alignment.TopEnd).size(48.dp)) {
            Icon(if(vm.scaleLinked)Icons.Default.Link else Icons.Default.LinkOff,"锁定缩放比例",tint=Accent,modifier=Modifier.size(18.dp))
        }
    }
}

private val easeNames=listOf("linear" to "线性","in" to "缓入","out" to "缓出","in_out" to "缓入缓出","hold" to "保持")
private fun easingValue(mode:String,t:Float)=when(mode){"in"->t*t;"out"->1-(1-t)*(1-t);"in_out"->t*t*(3-2*t);"hold"->if(t<1)0f else 1f;else->t}
@Composable private fun CurveEditor(vm:EditorViewModel,modifier:Modifier) {
    val segment=vm.easingSegment()
    val selected=segment?.first?.optString("ease")?:"linear"
    Column(modifier) {
        Text(segment?.let{"帧 "+it.first.getInt("frame")+" → "+it.second.getInt("frame")}?:"请移到两个关键帧之间",color=Muted,fontSize=10.sp,lineHeight=14.sp)
        Canvas(Modifier.weight(1f).fillMaxWidth().padding(12.dp).testTag("easing-graph")) {
            for(i in 0..4) {
                drawLine(Muted.copy(alpha=.16f),Offset(0f,size.height*i/4),Offset(size.width,size.height*i/4),1f)
                drawLine(Muted.copy(alpha=.16f),Offset(size.width*i/4,0f),Offset(size.width*i/4,size.height),1f)
            }
            val path=Path()
            for(i in 0..60) {
                val t=i/60f;val x=t*size.width;val y=(1-easingValue(selected,t))*size.height
                if(i==0)path.moveTo(x,y) else path.lineTo(x,y)
            }
            drawPath(path,Accent,style=Stroke(2.dp.toPx()))
        }
        Row(Modifier.fillMaxWidth().height(48.dp).horizontalScroll(rememberScrollState())) {
            easeNames.forEach{(key,label)->TextButton(onClick={vm.ease(key)},enabled=segment!=null,modifier=Modifier.height(48.dp)) {
                Text(label,fontSize=11.sp,lineHeight=14.sp,maxLines=1,color=if(selected==key)Accent else Muted)
            }}
        }
    }
}
