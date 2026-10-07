package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.floor
import kotlin.math.round
import java.util.Locale

private val vectorLabels=mapOf("corner_ratio" to "圆角比例","points" to "顶点数量","inner_ratio" to "内径比例","start_angle" to "起始角度","sweep_angle" to "展开角度","angle" to "形状角度","shaft_ratio" to "箭杆比例","head_ratio" to "箭头比例","inset_ratio" to "内缩比例","slant_ratio" to "倾斜比例","petal_ratio" to "花瓣比例")

@OptIn(ExperimentalMaterial3Api::class)
@Composable internal fun ShapeCatalogue(vm:EditorViewModel,onDismiss:()->Unit) {
    var query by remember{mutableStateOf("")};var category by remember{mutableStateOf("all")}
    ModalBottomSheet(onDismissRequest=onDismiss,containerColor=Panel,sheetState=rememberModalBottomSheetState(skipPartiallyExpanded=true)) {
        Column(Modifier.fillMaxWidth().heightIn(max=560.dp).padding(horizontal=16.dp).padding(bottom=16.dp).testTag("shape-catalogue")) {
            Row(verticalAlignment=Alignment.CenterVertically){Text("添加形状",Modifier.weight(1f),fontSize=20.sp);Tool(Icons.Default.Close,"关闭形状选择",action=onDismiss)}
            OutlinedTextField(query,{query=it},singleLine=true,placeholder={Text("搜索形状")},modifier=Modifier.fillMaxWidth().testTag("shape-search"))
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState())) {
                listOf("all" to "全部","basic" to "基础","polygons" to "多边形","symbols" to "符号","curves" to "曲线").forEach{(id,name)->
                    TextButton(onClick={category=id},modifier=Modifier.height(48.dp).semantics{selected=category==id}){Text(name,color=if(category==id)Accent else Muted)}
                }
            }
            Column(Modifier.weight(1f,false).verticalScroll(rememberScrollState())) {
                vm.shapeCatalogue().filter{(category=="all"||it.getString("category")==category)&&(query.isBlank()||it.getString("name").contains(query,true)||it.getString("english_name").contains(query,true))}.chunked(2).forEach{row->
                    Row(Modifier.fillMaxWidth()) {row.forEach{shape->TextButton(onClick={vm.addVectorShape(shape.getString("id"),shape.getString("name"));onDismiss()},modifier=Modifier.weight(1f).heightIn(min=52.dp).testTag("shape-${shape.getString("id")}")){Text(shape.getString("name"),color=Ink)}};if(row.size==1)Spacer(Modifier.weight(1f))}
                }
            }
        }
    }
}

@Composable internal fun VectorPanel(vm:EditorViewModel,modifier:Modifier,onCurveMode:(Boolean)->Unit) {
    val vector=vm.vectorData()?:return
    val source=vector.getJSONObject("source")
    var curve by remember(vm.root,vm.selected){mutableStateOf(false)}
    var convert by remember{mutableStateOf(false)}
    LaunchedEffect(curve){onCurveMode(curve)}
    DisposableEffect(Unit){onDispose{onCurveMode(false)}}
    Column(modifier.background(Panel).testTag("vector-panel")) {
        Row(Modifier.fillMaxWidth().heightIn(min=48.dp).padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically) {
            if(curve)TextButton(onClick={curve=false}){Text("返回属性")}else {
                listOf("geometry" to if(source.getString("kind")=="shape")"形状"else"路径","style" to "样式").forEach{(id,label)->
                    TextButton(onClick={vm.vectorTab=id},modifier=Modifier.testTag("vector-tab-$id").semantics{selected=vm.vectorTab==id}){Text(label,color=if(vm.vectorTab==id)Accent else Muted)}
                }
            }
            Spacer(Modifier.weight(1f));Tool(Icons.Default.Close,"收起矢量属性",action=vm::closeWorkspace)
        }
        if(curve)CurveEditor(vm,Modifier.weight(1f).fillMaxWidth())else {
            Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal=12.dp)) {
                if(vm.vectorTab=="style")VectorPaint(vm,vector)
                else if(source.getString("kind")=="shape") {
                    val shape=vm.shapeCatalogue().firstOrNull{it.getString("id")==source.getString("shape")}
                    shape?.optJSONArray("parameters").objects().forEach{p->
                        VectorNumber(vm,"vector:parameter:${p.getString("id")}",vectorLabels[p.getString("id")]?:p.getString("id"),p.getDouble("min"),p.getDouble("max"),p.optBoolean("discrete"))
                    }
                    TextButton(onClick={convert=true},enabled=vm.editable(),modifier=Modifier.testTag("vector-convert")){Text("转换为可编辑路径")}
                }else VectorPaths(vm,source)
            }
            val discrete=vm.property.startsWith("vector:parameter:")&&vm.shapeCatalogue().firstOrNull{it.getString("id")==source.optString("shape")}
                ?.optJSONArray("parameters").objects().firstOrNull{it.getString("id")==vm.property.substringAfterLast(':')}?.optBoolean("discrete")==true
            Row(Modifier.fillMaxWidth().height(48.dp).horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.SpaceEvenly) {
                Tool(Icons.Default.SkipPrevious,"上一关键帧",vm.keys().any{it.getInt("frame")<vm.frame}){vm.jumpKey(false)}
                Box(Modifier.testTag("vector-key")){Tool(if(vm.currentKey()!=null)Icons.Default.Diamond else Icons.Default.Add,"添加或删除矢量关键帧",vm.editable()&&vm.vectorTrackRaw(vm.selected,vm.property)!=null,vm::toggleKey)}
                Tool(Icons.Default.SkipNext,"下一关键帧",vm.keys().any{it.getInt("frame")>vm.frame}){vm.jumpKey(true)}
                TextButton(onClick={curve=true},enabled=!discrete&&vm.easingSegment()!=null&&vm.editable(),modifier=Modifier.testTag("vector-curve")){Text(if(discrete)"保持插值"else"曲线")}
            }
        }
    }
    if(convert)AlertDialog(onDismissRequest={convert=false},title={Text("转换为路径？")},text={Text("使用当前帧的形状生成节点。形状参数动画将固定为当前外观，填充与描边动画会保留。转换后可撤销。")},
        confirmButton={TextButton(onClick={convert=false;vm.vectorDrawMode=false;vm.vectorAction(JSONObject().put("action","convert_to_path").put("frame",floor(vm.frame).toInt()))},modifier=Modifier.testTag("vector-convert-confirm")){Text("转换")}},dismissButton={TextButton(onClick={convert=false}){Text("取消")}})
}

@Composable private fun VectorNumber(vm:EditorViewModel,key:String,label:String,min:Double,max:Double,discrete:Boolean=false,axis:Int?=null) {
    val sample=vm.vectorValue(vm.selected,key)?:return
    val value=if(axis==null)(sample as? Number)?.toDouble()?:return else (sample as? JSONArray)?.optDouble(axis)?:return
    var draft by remember(vm.selected,key,axis){mutableStateOf<Double?>(null)}
    var captured by remember{mutableStateOf<Any?>(null)};var at by remember{mutableIntStateOf(0)}
    var input by remember{mutableStateOf(false)}
    fun command(next:Double):JSONObject {
        val bounded=if(discrete)round(next.coerceIn(min,max))else next.coerceIn(min,max)
        val nextValue=if(axis==null)bounded else JSONArray((captured?:sample).toString()).put(axis,bounded)
        return JSONObject().put("op",if(axis==null)"set_scalar"else"set_vector").put("object",vm.selected).put("property",key).put("frame",at).put("value",nextValue)
    }
    Row(Modifier.fillMaxWidth().heightIn(min=56.dp),verticalAlignment=Alignment.CenterVertically) {
        TextButton(onClick={vm.selectVectorTrack(key)},modifier=Modifier.width(100.dp).heightIn(min=48.dp).testTag("vector-select-${key.substringAfter("vector:")}-${axis?:0}")) {
            Text(label,color=if(vm.property==key)Accent else Ink,fontSize=13.sp,maxLines=2,overflow=TextOverflow.Ellipsis)
        }
        NumericWheel(draft?:value,min,max,label,vm.editable(),Modifier.weight(1f).testTag("vector-wheel-${key.substringAfter("vector:")}-${axis?:0}"),
            onValueChange={next->if(captured==null){vm.selectVectorTrack(key);captured=sample;at=floor(vm.frame).toInt();vm.beginGesture()};draft=if(discrete)round(next)else next;vm.edit(command(next),false)},
            onFinished={if(captured!=null)vm.endGesture{draft=null};captured=null},onCancelled={if(captured!=null)vm.cancelGesture();captured=null;draft=null},inertiaGroup=vm.gestureInertia)
        TextButton(onClick={vm.gestureInertia.stop();input=true},enabled=vm.editable(),modifier=Modifier.widthIn(min=72.dp).heightIn(min=48.dp).testTag("vector-value-${key.substringAfter("vector:")}-${axis?:0}")) {
            Text(String.format(Locale.US,if(discrete)"%.0f"else"%.2f",draft?:value),fontSize=12.sp)
        }
    }
    if(input)InputDialog(label,value.toString(),{input=false},true){raw->val next=raw.toDoubleOrNull();if(next==null||!next.isFinite()||next !in min..max)vm.showOperationError("请输入 $min 到 $max 之间的数值")else{at=floor(vm.frame).toInt();vm.selectVectorTrack(key);vm.edit(command(next));input=false}}
}

@Composable private fun VectorPaint(vm:EditorViewModel,vector:JSONObject) {
    fun change(block:(JSONObject)->Unit){val next=JSONObject(vector.toString());block(next);vm.replaceVector(next)}
    fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    Row(verticalAlignment=Alignment.CenterVertically){Text("填充",Modifier.weight(1f));Switch(vector.optJSONObject("fill")!=null,{enabled->change{it.put("fill",if(enabled)track(JSONArray(listOf(.33,.86,.78,1)))else JSONObject.NULL)}},enabled=vm.editable(),modifier=Modifier.testTag("vector-fill-toggle"))}
    if(vector.optJSONObject("fill")!=null)listOf("红","绿","蓝","不透明度").forEachIndexed{axis,label->VectorNumber(vm,"vector:fill","填充 · $label",0.0,1.0,axis=axis)}
    Row(verticalAlignment=Alignment.CenterVertically){Text("描边",Modifier.weight(1f));Switch(vector.optJSONObject("stroke")!=null,{enabled->change{it.put("stroke",if(enabled)JSONObject().put("color",track(JSONArray(listOf(1,1,1,1)))).put("width",track(3)).put("cap","round").put("join","round").put("miter_limit",4)else JSONObject.NULL)}},enabled=vm.editable(),modifier=Modifier.testTag("vector-stroke-toggle"))}
    vector.optJSONObject("stroke")?.let{stroke->
        VectorNumber(vm,"vector:stroke_width","描边宽度",0.0,4096.0)
        listOf("红","绿","蓝","不透明度").forEachIndexed{axis,label->VectorNumber(vm,"vector:stroke_color","描边 · $label",0.0,1.0,axis=axis)}
        VectorOptions("端点",stroke.getString("cap"),listOf("butt" to "平头","round" to "圆头","square" to "方头"),vm.editable()){id->change{it.getJSONObject("stroke").put("cap",id)}}
        VectorOptions("拐角",stroke.getString("join"),listOf("miter" to "尖角","round" to "圆角","bevel" to "斜角"),vm.editable()){id->change{it.getJSONObject("stroke").put("join",id)}}
        if(stroke.getString("join")=="miter") {
            var input by remember{mutableStateOf(false)}
            TextButton(onClick={input=true},enabled=vm.editable()){Text("尖角限制 · ${stroke.getDouble("miter_limit")}")}
            if(input)InputDialog("尖角限制",stroke.getDouble("miter_limit").toString(),{input=false},true){raw->val value=raw.toDoubleOrNull();if(value!=null&&value.isFinite()&&value in 1.0..100.0){change{it.getJSONObject("stroke").put("miter_limit",value)};input=false}else vm.showOperationError("尖角限制范围为 1 到 100")}
        }
    }
    VectorOptions("填充规则",vector.getString("fill_rule"),listOf("non_zero" to "非零","even_odd" to "奇偶"),vm.editable()){id->change{it.put("fill_rule",id)}}
}

@Composable private fun VectorOptions(label:String,chosen:String,items:List<Pair<String,String>>,enabled:Boolean,onChoose:(String)->Unit) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically){Text(label,color=Muted,fontSize=13.sp);items.forEach{(id,name)->TextButton(onClick={onChoose(id)},enabled=enabled,modifier=Modifier.heightIn(min=48.dp).semantics{selected=chosen==id}){Text(name,color=if(chosen==id)Accent else Muted)}}}
}

@Composable private fun VectorPaths(vm:EditorViewModel,source:JSONObject) {
    val paths=source.optJSONArray("paths").objects()
    LaunchedEffect(paths.map{it.getLong("id")}){if(paths.none{it.getLong("id")==vm.vectorPathId})vm.vectorPathId=paths.firstOrNull()?.getLong("id")?:1}
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically){paths.forEachIndexed{i,path->TextButton(onClick={vm.vectorPathId=path.getLong("id");vm.vectorNodeId=0}){Text("路径 ${i+1}",color=if(vm.vectorPathId==path.getLong("id"))Accent else Muted)}};TextButton(onClick=vm::addVectorPath,enabled=vm.editable()&&paths.size<64,modifier=Modifier.testTag("vector-new-path")){Text("＋路径")}}
    val path=paths.firstOrNull{it.getLong("id")==vm.vectorPathId}?:return
    VectorOptions("操作",if(vm.vectorDrawMode)"draw"else"edit",listOf("draw" to "加点","edit" to "编辑"),vm.editable()){vm.vectorDrawMode=it=="draw"}
    Text(if(vm.vectorDrawMode)"在预览中点按加点，拖动生成曲柄。"else"点选节点，拖动节点或曲柄。",color=Muted,fontSize=12.sp)
    Row(verticalAlignment=Alignment.CenterVertically){Text("闭合路径",Modifier.weight(1f));Switch(path.getBoolean("closed"),{next->vm.updateVectorPaths{it.first{p->p.getLong("id")==vm.vectorPathId}.put("closed",next)}},enabled=vm.editable()&&path.getJSONArray("nodes").length()>=3,modifier=Modifier.testTag("vector-close-path"))}
    val node=path.getJSONArray("nodes").objects().firstOrNull{it.getLong("id")==vm.vectorNodeId}
    if(node!=null) {
        val key="vector:node:${vm.vectorPathId}:${vm.vectorNodeId}"
        VectorOptions("曲柄",vm.vectorHandleMode,listOf("corner" to "独立","smooth" to "平滑","symmetric" to "对称"),vm.editable()){vm.vectorHandleMode=it}
        listOf("节点 X","节点 Y","入柄 X","入柄 Y","出柄 X","出柄 Y").forEachIndexed{axis,label->VectorNumber(vm,key,label,-32768.0,32768.0,axis=axis)}
        TextButton(onClick={vm.updateVectorPaths{list->list.first{it.getLong("id")==vm.vectorPathId}.let{p->p.put("nodes",JSONArray(p.getJSONArray("nodes").objects().filter{it.getLong("id")!=vm.vectorNodeId}));if(p.getJSONArray("nodes").length()<3)p.put("closed",false)}};vm.vectorNodeId=0},enabled=vm.editable(),modifier=Modifier.testTag("vector-delete-node")){Text("删除节点")}
    }else Text("选择一个节点以调整位置、曲柄与动画。",color=Muted,fontSize=12.sp)
    TextButton(onClick={vm.updateVectorPaths{it.removeAll{p->p.getLong("id")==vm.vectorPathId}};vm.vectorNodeId=0},enabled=vm.editable(),modifier=Modifier.testTag("vector-delete-path")){Text("删除当前路径")}
}
