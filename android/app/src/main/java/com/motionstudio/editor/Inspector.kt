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
import androidx.compose.material.icons.automirrored.filled.RotateRight
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import java.util.Locale
import kotlin.math.*

@Composable internal fun EditorFooter(vm:EditorViewModel) {
    if(vm.layerSelectionMode){LayerSelectionBar(vm);return}
    var more by remember(vm.root,vm.compositionId,vm.selected){mutableStateOf(false)}
    var precompose by remember{mutableStateOf(false)}
    val kind=vm.contentKind()
    val hasTarget=vm.selected!=0L||vm.hasCamera()
    BoxWithConstraints(Modifier.fillMaxWidth().height(52.dp).background(Panel)
        .then(if(vm.panelOpen)Modifier.clearAndSetSemantics{}else Modifier).testTag("editor-footer")) {
        val nameWidth=(maxWidth*.24f).coerceIn(64.dp,100.dp)
        Row(Modifier.fillMaxSize().padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically) {
            Text(if(hasTarget)objectName(vm,vm.selected)else"选择图层",
                modifier=if(hasTarget)Modifier.width(nameWidth).padding(end=8.dp)else Modifier.weight(1f),
                color=Muted,fontSize=13.sp,lineHeight=20.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
            if(hasTarget)key(vm.root,vm.compositionId,vm.selected) {
                Row(Modifier.weight(1f).fillMaxHeight().horizontalScroll(rememberScrollState()).testTag("layer-categories"),
                    verticalAlignment=Alignment.CenterVertically) {
                    if(kind=="audio")LayerCategory("声音","footer-audio"){vm.openProperty("audio")}
                    else {
                        LayerCategory("变换","footer-transform") {
                            vm.openProperty(if(vm.property in listOf("position","rotation","scale","opacity"))vm.property else "position")
                        }
                        if(kind in listOf("solid","image","text","video","vector","adjustment","composition"))LayerCategory("效果","open-effects",vm::openEffects)
                        if(kind in listOf("solid","image","text","video","vector","composition"))LayerCategory("蒙版","open-masks",vm::openMasks)
                        if(kind=="vector") {
                            LayerCategory(if(vm.vectorData()?.getJSONObject("source")?.optString("kind")=="shape")"形状"else"路径","footer-vector"){vm.openVector()}
                            LayerCategory("样式","footer-vector-style"){vm.openVector("style")}
                        }
                        if(vm.audioClip()!=null)LayerCategory("原声","footer-audio"){vm.openProperty("audio")}
                        if(kind=="composition") {
                            LayerCategory("子合成","footer-composition",vm::openSelectedComposition)
                            LayerCategory("片段","footer-composition-clip",vm::openCompositionClip)
                        }
                        if(vm.selected==0L) {
                            LayerCategory("镜头","footer-lens"){vm.openProperty("fov")}
                            LayerCategory("目标点","footer-target"){vm.openProperty("target")}
                        }
                    }
                }
            }
            Box {
                Tool(Icons.Default.MoreHoriz,"图层快捷操作"){more=true}
                DropdownMenu(more,{more=false}) {
                    DropdownMenuItem(text={Text("多选图层")},modifier=Modifier.testTag("start-layer-selection"),enabled=vm.state.project?.optJSONArray("layers")?.length()?.let{it>0}==true,onClick={more=false;vm.startLayerSelection()})
                    if(vm.selected!=0L) {
                        DropdownMenuItem(text={Text("创建副本")},enabled=vm.editable(),onClick={more=false;vm.duplicate()})
                        DropdownMenuItem(text={Text("预合成")},enabled=vm.canPrecompose(),modifier=Modifier.testTag("precompose-layer"),onClick={more=false;precompose=true})
                        DropdownMenuItem(text={Text("删除图层")},enabled=vm.editable(),onClick={more=false;vm.deleteLayer()})
                    }
                }
            }
        }
    }
    if(precompose)InputDialog("预合成名称","预合成",{precompose=false}){vm.precompose(it);precompose=false}
}

@Composable private fun LayerCategory(label:String,tag:String,onClick:()->Unit) {
    TextButton(onClick=onClick,contentPadding=PaddingValues(horizontal=12.dp,vertical=0.dp),
        modifier=Modifier.widthIn(min=64.dp).height(48.dp).testTag(tag)) {
        Text(label,color=Ink,fontSize=14.sp,lineHeight=20.sp,maxLines=1)
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
    private val linked=vm.scaleLinked
    fun adjust(axis:Int,amount:Double) {
        offsets[axis]+=amount*stepFor(key)
        publish(axis,listOf(axis))
    }
    fun adjustXY(x:Double,y:Double) {
        offsets[0]+=x*stepFor(key);offsets[1]+=y*stepFor(key)
        publish(0,listOf(0,1))
    }
    private fun publish(axis:Int,axes:List<Int>) {
        if(original is JSONArray) {
            val value=JSONArray(original.toString())
            for(i in 0..2)value.put(i,bounded(key,original.getDouble(i)+offsets[i]))
            if(key=="scale"&&linked&&axis<2) {
                val other=1-axis;val base=original.getDouble(axis)
                value.put(other,bounded(key,if(abs(base)>.0001)original.getDouble(other)*value.getDouble(axis)/base else value.getDouble(axis)))
            }
            vm.setPropertyValue(objectId,key,frame,value,false,if(key=="scale"&&linked&&axis<2)listOf(0,1)else axes)
        } else if(original is Number)vm.setPropertyValue(objectId,key,frame,bounded(key,original.toDouble()+offsets[0]),false)
    }
}

@Composable internal fun Properties(vm:EditorViewModel,modifier:Modifier,backEnabled:Boolean=true,onCurveMode:(Boolean)->Unit={}) {
    if(vm.contentKind()=="audio"||vm.property=="audio") {AudioProperties(vm,modifier);return}
    var rename by remember{mutableStateOf(false)}
    var anchor by remember{mutableStateOf(false)}
    var more by remember{mutableStateOf(false)}
    var parenting by remember{mutableStateOf(false)}
    var curves by remember(vm.selected,vm.property){mutableStateOf(false)}
    LaunchedEffect(curves){onCurveMode(curves)}
    DisposableEffect(Unit){onDispose{onCurveMode(false)}}
    BackHandler(enabled=backEnabled&&curves){curves=false}
    val camera=vm.selected==0L
    val threeD=vm.threeD()
    LaunchedEffect(vm.selected,vm.property,threeD){if(vm.activeAxis() !in vm.visibleAxes())vm.chooseAxis(vm.visibleAxes().first())}
    val mode=vm.state.project?.optJSONObject("camera")?.optString("mode")?:"position"
    val choices=if(camera) {
        if(mode=="orbit")listOf("radius" to "距离","azimuth" to "方位","elevation" to "俯仰","target" to "目标点","fov" to "视角","roll" to "滚转")
        else listOf("position" to "位置","target" to "目标点","fov" to "视角","roll" to "滚转")
    } else listOf("position" to "位置","rotation" to "旋转","scale" to "缩放","opacity" to "透明度")
    Column(modifier.testTag("properties-panel").background(Panel,RoundedCornerShape(topStart=16.dp,topEnd=16.dp))
        .pointerInput(Unit){awaitPointerEventScope{while(true)awaitPointerEvent()}}.padding(horizontal=8.dp)) {
        BoxWithConstraints(Modifier.fillMaxWidth().height(48.dp)) {
        val compact=maxWidth<360.dp||LocalDensity.current.fontScale>1.3f
        Row(Modifier.fillMaxSize(),verticalAlignment=Alignment.CenterVertically) {
            if(curves)Tool(Icons.AutoMirrored.Filled.ArrowBack,"返回变换参数"){curves=false}
            Text(if(curves)if(vm.isSeparated())vm.axisName().uppercase()+" 轴曲线"else"缓动曲线" else objectName(vm,vm.selected),
                Modifier.weight(1f),color=Ink,fontSize=15.sp,fontWeight=FontWeight.SemiBold,maxLines=1,overflow=TextOverflow.Ellipsis)
            if(!curves&&!camera&&vm.contentKind()!="adjustment")TextButton(onClick={vm.setThreeD(!threeD)},enabled=vm.editable(),
                modifier=Modifier.width(48.dp).height(48.dp).testTag("layer-3d-toggle").semantics{selected=threeD;stateDescription=if(threeD)"3D 图层"else"2D 图层"},contentPadding=PaddingValues(0.dp)) {
                Text(if(threeD)"3D"else"2D",fontSize=12.sp,color=if(threeD)Accent else Muted)
            }
            if(!curves)TextButton(onClick={curves=true},modifier=Modifier.height(48.dp).then(if(compact)Modifier.width(48.dp)else Modifier).testTag("open-curves"),
                contentPadding=PaddingValues(if(compact)0.dp else 12.dp)) {
                Icon(editorIcon(Icons.AutoMirrored.Filled.ShowChart),"缓动曲线",Modifier.size(18.dp),tint=Accent)
                if(!compact){Spacer(Modifier.width(4.dp));Text(if(vm.isSeparated())vm.axisName().uppercase()+" 曲线"else if(vm.sampleValue() is JSONArray)"整体曲线"else"曲线",color=Accent,fontSize=12.sp)}
            }
            Box {
                Tool(Icons.Default.MoreVert,"图层操作"){more=true}
                DropdownMenu(more,{more=false}) {
                    vm.expressionTargetForCurrent(if(vm.isSeparated())vm.activeAxis()else null)?.let{target->
                        DropdownMenuItem(text={Text("表达式")},modifier=Modifier.testTag("open-property-expression"),onClick={more=false;vm.openExpression(target)})
                    }
                    if(vm.canSeparate())DropdownMenuItem(text={Text("分离 XYZ")},modifier=Modifier.testTag("separate-dimensions"),enabled=vm.editable(),onClick={more=false;vm.separateDimensions()})
                    DropdownMenuItem(text={Text("父级")},enabled=vm.editable(),onClick={more=false;parenting=true})
                    if(camera) {
                        DropdownMenuItem(text={Text(if(mode=="orbit")"切换位置路径" else "切换环绕轨道")},onClick={more=false;vm.cameraMode(mode!="orbit");vm.property=if(mode=="orbit")"position" else "radius"})
                        DropdownMenuItem(text={Text("删除摄影机")},onClick={more=false;vm.deleteLayer()})
                    }
                    else {
                        if(vm.contentKind() in listOf("solid","image","text","video"))DropdownMenuItem(text={Text("效果")},modifier=Modifier.testTag("open-effects"),onClick={more=false;vm.openEffects()})
                        if(vm.audioClip()!=null)DropdownMenuItem(text={Text("原声")},onClick={more=false;vm.property="audio"})
                        DropdownMenuItem(text={Text("锚点")},enabled=vm.editable(),onClick={more=false;anchor=true})
                        DropdownMenuItem(text={Text("摄影机对准此图层")},onClick={more=false;vm.focusCameraOnSelection()})
                        DropdownMenuItem(text={Text("重命名")},onClick={more=false;rename=true})
                        DropdownMenuItem(text={Text("创建副本")},onClick={more=false;vm.duplicate()})
                        DropdownMenuItem(text={Text("上移图层")},onClick={more=false;vm.reorder(1)})
                        DropdownMenuItem(text={Text("下移图层")},onClick={more=false;vm.reorder(-1)})
                        DropdownMenuItem(text={Text("锁定 / 解锁")},onClick={more=false;vm.layer(vm.selected)?.let{vm.flags(vm.selected,it.getBoolean("visible"),!it.getBoolean("locked"))}})
                        DropdownMenuItem(text={Text("删除图层")},onClick={more=false;vm.deleteLayer()})
                    }
                    if(vm.keys().isNotEmpty())DropdownMenuItem(text={Text(if(vm.isSeparated())"移除 "+vm.axisName().uppercase()+" 轴动画"else"移除此属性动画")},enabled=vm.editable(),onClick={more=false;vm.animate()})
                }
            }
            Tool(Icons.Default.Close,"关闭属性面板"){vm.panelOpen=false}
        }}
        HorizontalDivider(color=Muted.copy(alpha=.10f))
        if(curves)key(vm.selected,vm.property,if(vm.isSeparated())vm.activeAxis()else -1){CurveEditor(vm,Modifier.weight(1f).fillMaxWidth())}
        else {
            BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            val fontScale=LocalDensity.current.fontScale
            val rotationHeight=96.dp+maxOf(48.dp,(34*fontScale+12).dp)
            val valueHeight=48.dp+96.dp+maxOf(56.dp,(34*fontScale+12).dp)
            val scrollValues=fontScale>1.3f||maxHeight<if(vm.property=="rotation")rotationHeight else valueHeight
            Column(Modifier.fillMaxSize().testTag("property-values")
                .then(if(scrollValues)Modifier.verticalScroll(rememberScrollState())else Modifier)) {
                Row(Modifier.fillMaxWidth().height(48.dp).horizontalScroll(rememberScrollState())) {
                    choices.forEach{(key,label)->TextButton(onClick={vm.pause();vm.property=key},modifier=Modifier.height(48.dp)
                        .testTag("property-tab-"+key).semantics{selected=vm.property==key;role=Role.Tab}) {
                        Column(horizontalAlignment=Alignment.CenterHorizontally) {
                            Text(label,fontSize=13.sp,lineHeight=18.sp,maxLines=1,color=if(vm.property==key)Accent else Muted)
                            Spacer(Modifier.height(4.dp))
                            Box(Modifier.height(2.dp).width(24.dp).background(if(vm.property==key)Accent else Color.Transparent))
                        }
                    }}
                }
                SourceColorProperty(vm)
                if(vm.property=="rotation") {
                    if(!scrollValues)Spacer(Modifier.weight(1f))
                    RotationRuler(vm,Modifier.height(48.dp).fillMaxWidth())
                }
                val value=vm.sampleValue()
                if(value is JSONArray)Row(Modifier.fillMaxWidth(),horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                    for(i in vm.visibleAxes().filter{it<value.length()})ScrubField(vm,listOf("X","Y","Z")[i],value.getDouble(i),i,Modifier.weight(1f),
                        compact=vm.property=="rotation",active=(vm.isSeparated()||vm.property=="rotation")&&vm.activeAxis()==i,
                        onFocus={vm.chooseAxis(i)})
                } else if(value is Number)ScrubField(vm,choices.firstOrNull{it.first==vm.property}?.second?:"数值",value.toDouble(),0,Modifier.fillMaxWidth())
                if(vm.property=="rotation"&&!scrollValues)Spacer(Modifier.weight(1f))
                if(vm.property!="rotation")TransformPad(vm,(if(scrollValues)Modifier.height(96.dp)else Modifier.weight(1f)).fillMaxWidth().padding(vertical=4.dp))
            }
            }
            Row(Modifier.fillMaxWidth().height(48.dp),verticalAlignment=Alignment.CenterVertically) {
                val parent=parentOf(vm,vm.selected)
                TextButton(onClick={parenting=true},enabled=vm.editable(),modifier=Modifier.weight(1f).height(48.dp).testTag("open-parent"),contentPadding=PaddingValues(horizontal=8.dp)) {
                    Icon(editorIcon(if(parent==null)Icons.Default.LinkOff else Icons.Default.Link),null,Modifier.size(18.dp),tint=if(parent==null)Muted else Accent)
                    Spacer(Modifier.width(6.dp))
                    Text("父级 · "+(parent?.let{objectName(vm,it)}?:"未绑定"),color=if(parent==null)Muted else Accent,fontSize=12.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                }
                Tool(Icons.Default.SkipPrevious,"上一关键帧",vm.keys().any{it.getInt("frame")<vm.frame}){vm.jumpKey(false)}
                Box(Modifier.testTag("property-key")) {
                    Tool(Icons.Default.Diamond,if(vm.currentKey()==null)"添加关键帧" else "删除当前关键帧",vm.editable(),vm::toggleKey)
                    Text(if(vm.currentKey()==null)"+" else "−",Modifier.align(Alignment.Center),color=Background,fontSize=13.sp)
                }
                Tool(Icons.Default.SkipNext,"下一关键帧",vm.keys().any{it.getInt("frame")>vm.frame}){vm.jumpKey(true)}
            }
        }
    }
    if(rename)InputDialog("图层名称",vm.layer(vm.selected)?.optString("name")?:"",onDismiss={rename=false}){vm.rename(it);rename=false}
    if(anchor)AnchorDialog(vm){anchor=false}
    if(parenting)ParentSheet(vm){parenting=false}
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

@Composable private fun ScrubField(vm:EditorViewModel,label:String,value:Double,axis:Int,modifier:Modifier,
    compact:Boolean=false,active:Boolean=false,onFocus:()->Unit={}) {
    var editing by remember(vm.selected,vm.property){mutableStateOf(false)}
    var drag by remember{mutableStateOf<ValueDrag?>(null)}
    val density=LocalDensity.current.density
    val factor=if(vm.property=="opacity")100.0 else 1.0
    val key=vm.property;val objectId=vm.selected;val frame=floor(vm.frame).toInt()
    Surface(color=Background,shape=RoundedCornerShape(8.dp),modifier=modifier.heightIn(min=if(compact)48.dp else 56.dp).testTag("value-"+label)
        .pointerInput(vm.selected,key,axis,vm.editable(),density) {
            if(vm.editable())detectHorizontalDragGestures(
                onDragStart={onFocus();drag=ValueDrag(vm);vm.beginGesture()},
                onDragEnd={vm.endGesture();drag=null},onDragCancel={vm.cancelGesture();drag=null}){change,amount->
                    change.consume();drag?.adjust(axis,amount/density.toDouble())
                }
        }
        .clickable(enabled=vm.editable()){vm.pause();onFocus();editing=true}) {
        Column(Modifier.padding(horizontal=6.dp,vertical=6.dp),verticalArrangement=Arrangement.Center) {
            Text(label+(if(key=="scale"||key=="opacity")" %" else if(key=="rotation"||key=="fov"||key=="roll")" °" else ""),color=if(active)Accent else Muted,fontSize=11.sp,lineHeight=14.sp,maxLines=1)
            Text(String.format(Locale.US,"%.1f",value*factor),modifier=Modifier.testTag("number-"+label),color=if(vm.editable())Ink else Muted,
                fontSize=16.sp,lineHeight=20.sp,fontFamily=FontFamily.Monospace,maxLines=1,overflow=TextOverflow.Ellipsis)
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
            vm.setPropertyValue(objectId,key,frame,next,editedAxes=if(key=="scale"&&vm.scaleLinked&&axis<2)listOf(0,1)else listOf(axis));editing=false
        }
    }
}

/** Rotation is a one-dimensional angle edit. Axis choice is transient UI state;
 * a drag captures its object, axis, property and frame before any JNI reply. */
@Composable private fun RotationRuler(vm:EditorViewModel,modifier:Modifier) {
    var drag by remember{mutableStateOf<ValueDrag?>(null)}
    val axis=vm.rotationAxis
    val name=listOf("X","Y","Z")[axis]
    val value=(vm.sampleValue() as? JSONArray)?.optDouble(axis)?:0.0
    val density=LocalDensity.current
    Row(modifier.heightIn(min=48.dp).testTag("rotation-controls"),verticalAlignment=Alignment.CenterVertically) {
        PropertyAxisMenu(vm,"rotation-axis")
        Canvas(Modifier.weight(1f).fillMaxHeight().heightIn(min=48.dp).clipToBounds().testTag("rotation-ruler")
            .semantics{contentDescription="滑动调整 "+name+" 轴角度";stateDescription=String.format(Locale.US,"%.1f 度",value)}
            .pointerInput(vm.selected,vm.property,axis,vm.editable(),density.density) {
                if(vm.editable())detectHorizontalDragGestures(
                    onDragStart={drag=ValueDrag(vm);vm.beginGesture()},
                    onDragEnd={vm.endGesture();drag=null},
                    onDragCancel={vm.cancelGesture();drag=null}){change,amount->
                        change.consume();drag?.adjust(axis,amount/density.density.toDouble())
                    }
            }) {
            val pixelsPerDegree=4.dp.toPx()
            val low=floor((value-size.width/2/pixelsPerDegree)/5).toInt()*5
            val high=ceil((value+size.width/2/pixelsPerDegree)/5).toInt()*5
            val paint=android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
                color=android.graphics.Color.rgb(170,180,194);textAlign=android.graphics.Paint.Align.CENTER
                textSize=11.sp.toPx();typeface=android.graphics.Typeface.MONOSPACE
            }
            val widestLabel=maxOf(paint.measureText(low.toString()+"°"),paint.measureText(high.toString()+"°"))
            val majorInterval=ceil((widestLabel+12.dp.toPx())/pixelsPerDegree/15).toInt().coerceAtLeast(1)*15
            val top=(size.height-48.dp.toPx())/2
            for(degrees in low..high step 5) {
                val x=size.width/2+((degrees-value)*pixelsPerDegree).toFloat()
                val major=degrees%majorInterval==0
                drawLine(Muted.copy(alpha=if(vm.editable()).45f else .2f),Offset(x,top+5.dp.toPx()),Offset(x,top+(if(major)18.dp.toPx()else 12.dp.toPx())),1.dp.toPx())
                if(major)drawContext.canvas.nativeCanvas.drawText(degrees.toString()+"°",x,top+35.dp.toPx(),paint)
            }
            drawLine(if(vm.editable())Accent else Muted,Offset(size.width/2,top+2.dp.toPx()),Offset(size.width/2,top+20.dp.toPx()),1.5.dp.toPx())
        }
    }
}

@Composable internal fun PropertyAxisMenu(vm:EditorViewModel,prefix:String="property-axis") {
    var menu by remember{mutableStateOf(false)}
    val axis=vm.activeAxis()
    Box {
        TextButton(onClick={menu=true},modifier=Modifier.widthIn(min=56.dp).height(48.dp).testTag(prefix+"-menu"),contentPadding=PaddingValues(4.dp)) {
            Text(vm.axisName().uppercase()+"轴",color=Accent,fontSize=13.sp,maxLines=1)
            Icon(editorIcon(Icons.Default.ArrowDropDown),"选择属性轴",Modifier.size(14.dp),tint=Muted)
        }
        DropdownMenu(menu,{menu=false}) {
            vm.visibleAxes().forEach{i->val name=vm.axisName(i).uppercase()
                DropdownMenuItem(text={Text(name+" 轴"+if(vm.property=="rotation")"旋转"else"",color=if(axis==i)Accent else Ink)},
                    modifier=Modifier.testTag(prefix+"-"+name).semantics{selected=axis==i},onClick={vm.chooseAxis(i);menu=false})
            }
        }
    }
}

@Composable private fun TransformPad(vm:EditorViewModel,modifier:Modifier) {
    var drag:ValueDrag? by remember{mutableStateOf(null)}
    var zAxis by remember(vm.property){mutableStateOf(false)}
    LaunchedEffect(vm.threeD()){if(!vm.threeD())zAxis=false}
    val density=LocalDensity.current.density
    val vector=vm.sampleValue() is JSONArray
    val separated=vector&&vm.isSeparated()
    val axis=vm.activeAxis()
    val name=vm.axisName().uppercase()
    val spatial=vm.property in listOf("position","target")
    Box(modifier.background(Background,RoundedCornerShape(10.dp)).testTag("transform-pad")
        .pointerInput(vm.selected,vm.property,zAxis,vm.scaleLinked,vm.editable(),vm.threeD(),separated,axis) {
            if(vm.editable())detectDragGestures(onDragStart={drag=ValueDrag(vm);vm.beginGesture()},
                onDragEnd={vm.endGesture();drag=null},onDragCancel={vm.cancelGesture();drag=null}){change,amount->
                change.consume()
                if(separated) {
                    val delta=if(spatial)when(axis){1->amount.y;2->-amount.y;else->amount.x}else amount.x
                    drag?.adjust(axis,delta/density.toDouble())
                } else if(vector&&spatial) {
                    if(zAxis)drag?.adjust(2,-amount.y/density.toDouble())
                    else drag?.adjustXY(amount.x/density.toDouble(),amount.y/density.toDouble())
                } else drag?.adjust(0,(amount.x-amount.y)/density.toDouble())
            }
        }) {
        Canvas(Modifier.fillMaxSize()) {
            drawLine(Muted.copy(alpha=.10f),Offset(size.width/2,0f),Offset(size.width/2,size.height),1f)
            drawLine(Muted.copy(alpha=.10f),Offset(0f,size.height/2),Offset(size.width,size.height/2),1f)
            for(i in 1..7)drawLine(Muted.copy(alpha=.1f),Offset(size.width*i/8,0f),Offset(size.width*i/8,4.dp.toPx()),1f)
        }
        Column(Modifier.align(Alignment.Center),horizontalAlignment=Alignment.CenterHorizontally) {
        Text(if(!vm.editable())"图层已锁定" else when(vm.property) {
            "position","target"->if(separated)if(axis==0)"左右滑动调整 X"else"上下滑动调整 "+name
                else if(zAxis)"上下滑动调整 Z" else "滑动移动 · XY"
            "scale"->if(separated)"左右滑动缩放 "+name+(if(vm.scaleLinked&&axis<2)" · XY 联动"else"")else"滑动缩放"
            "opacity"->"滑动调整透明度";else->"滑动调节数值"
        },color=Muted,fontSize=12.sp,lineHeight=18.sp)
        }
        if(separated)Box(Modifier.align(Alignment.TopStart)){PropertyAxisMenu(vm)}
        if(!separated&&vm.threeD()&&spatial)TextButton(onClick={zAxis=!zAxis},modifier=Modifier.align(Alignment.TopEnd).size(48.dp).testTag("pad-z-toggle")) {
            Text(if(zAxis)"XY" else "Z",color=Accent,fontSize=11.sp,lineHeight=14.sp)
        }
        if(vm.property=="scale")IconButton(onClick={vm.scaleLinked=!vm.scaleLinked},modifier=Modifier.align(Alignment.TopEnd).size(48.dp)) {
            Icon(editorIcon(if(vm.scaleLinked)Icons.Default.Link else Icons.Default.LinkOff),"锁定缩放比例",tint=Accent,modifier=Modifier.size(18.dp))
        }
    }
}
