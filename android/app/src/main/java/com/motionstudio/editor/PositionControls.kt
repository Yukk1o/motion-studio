package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.OpenWith
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.*
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale

/** The same position control is hosted by transforms, effects and native slots. */
@OptIn(ExperimentalFoundationApi::class)
@Composable internal fun PositionControls(label:String,tag:String,value:JSONArray,axes:List<Int>,enabled:Boolean,
    minimum:Double,maximum:Double,onSelect:()->Unit={},onAxis:(Int)->Unit={},
    onBegin:()->Unit,onValue:(JSONArray,List<Int>)->Unit,onFinish:(Boolean)->Unit,
    inertiaGroup:GestureInertiaGroup?=null,separated:Boolean=false,initialAxis:Int=axes.firstOrNull()?:0) {
    var axis by remember(tag){mutableIntStateOf(initialAxis)}
    var pad by remember(tag){mutableStateOf(false)}
    var number by remember(tag){mutableStateOf<Int?>(null)}
    var captured by remember(tag){mutableStateOf<JSONArray?>(null)}
    var draft by remember(tag){mutableStateOf<JSONArray?>(null)}
    val shown=draft?:value
    val density=LocalDensity.current.density
    val currentValue by rememberUpdatedState(value)
    val currentBegin by rememberUpdatedState(onBegin)
    val currentChange by rememberUpdatedState(onValue)
    val currentFinish by rememberUpdatedState(onFinish)
    LaunchedEffect(axes){if(axis !in axes)axis=axes.firstOrNull()?:0}
    LaunchedEffect(initialAxis){if(initialAxis in axes)axis=initialAxis}
    fun begin() {if(captured==null){onSelect();captured=JSONArray(currentValue.toString());draft=JSONArray(currentValue.toString());currentBegin()}}
    fun set(next:JSONArray,changed:List<Int>) {begin();draft=next;currentChange(next,changed)}
    fun finish(commit:Boolean) {if(captured!=null)currentFinish(commit);captured=null;draft=null}
    DisposableEffect(tag){onDispose{if(captured!=null)currentFinish(false)}}
    Box(Modifier.fillMaxWidth().testTag("$tag-controls")) {
        Column {
            Row(Modifier.fillMaxWidth().height(56.dp),verticalAlignment=Alignment.CenterVertically) {
                TextButton(onClick=onSelect,modifier=Modifier.weight(.9f).heightIn(min=48.dp).testTag("$tag-select"),contentPadding=PaddingValues(2.dp)) {
                    Text(label,color=Ink,fontSize=13.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                }
                axes.forEach{i->val name=listOf("X","Y","Z")[i]
                    Surface(color=if(axis==i)Accent.copy(alpha=.15f)else Background,shape=RoundedCornerShape(8.dp),
                        modifier=Modifier.weight(1f).height(56.dp).testTag("$tag-axis-$name")
                            .semantics{selected=axis==i;contentDescription="$name · ${shown.optDouble(i)}";customActions=listOf(CustomAccessibilityAction("输入 $name"){number=i;true})}
                            .combinedClickable(enabled=enabled,onClick={inertiaGroup?.stop();onSelect();axis=i;onAxis(i)},onLongClick={onSelect();axis=i;onAxis(i);number=i})
                            .pointerInput(enabled,i,density){if(enabled)detectHorizontalDragGestures(onDragStart={inertiaGroup?.stop();axis=i;onAxis(i);begin()},onDragEnd={finish(true)},onDragCancel={finish(false)}){change,amount->
                                change.consume();begin();set(JSONArray(draft.toString()).put(i,(draft!!.optDouble(i)+amount/density).coerceIn(minimum,maximum)),listOf(i))
                            }}) {
                        Column(Modifier.fillMaxSize().then(if(tag=="position")Modifier.testTag("value-$name")else Modifier),verticalArrangement=Arrangement.Center,horizontalAlignment=Alignment.CenterHorizontally) {
                            Text(String.format(Locale.US,"%.1f",shown.optDouble(i)),Modifier.testTag(if(tag=="position")"number-$name"else"$tag-number-$name"),color=if(axis==i)Ink else Muted,fontSize=13.sp,maxLines=1,overflow=TextOverflow.Ellipsis)
                            Text(name,color=if(axis==i)Accent else Muted,fontSize=11.sp)
                        }
                    }
                }
                IconButton(onClick={inertiaGroup?.stop();onSelect();pad=true},enabled=enabled,modifier=Modifier.size(48.dp).testTag("$tag-pad-open")){Icon(Icons.Default.OpenWith,"打开位置触控板")}
            }
            Surface(color=Background,shape=RoundedCornerShape(10.dp),modifier=Modifier.fillMaxWidth()) {
                NumericWheel(shown.optDouble(axis),minimum,maximum,"${listOf("X","Y","Z")[axis]} 位置",enabled,
                    modifier=Modifier.fillMaxWidth().testTag("$tag-wheel"),inertiaGroup=inertiaGroup,
                    onValueChange={next->begin();set(JSONArray(draft.toString()).put(axis,next),listOf(axis))},
                    onFinished={finish(true)},onCancelled={finish(false)})
            }
        }
        if(pad)Box(Modifier.matchParentSize().then(if(tag=="position")Modifier.testTag("transform-pad")else Modifier)){TransformTouchpad(Modifier.fillMaxSize().testTag("$tag-pad"),axes,enabled,lockedAxis=axis.takeIf{separated},
            onBegin={begin()},onDelta={x,y,z->
                begin();val next=JSONArray(draft.toString())
                val changed=if(separated)listOf(axis)else if(z)listOf(2)else axes.filter{it<2}
                changed.forEach{i->next.put(i,(next.optDouble(i)+when(i){0->x;1->y;else->-y}).coerceIn(minimum,maximum))}
                set(next,changed)
            },onFinish={finish(it)},onBack={pad=false})}
    }
    number?.let{i->InputDialog("$label · ${listOf("X","Y","Z")[i]}",value.optDouble(i).toString(),onDismiss={number=null},numeric=true){raw->
        raw.toDoubleOrNull()?.takeIf{it.isFinite()&&it in minimum..maximum}?.let{v->
            begin();set(JSONArray(currentValue.toString()).put(i,v),listOf(i));finish(true);number=null
        }
    }}
}

@Composable internal fun EffectPositionControls(vm:EditorViewModel,objectId:Long,instance:Long,desc:JSONObject,value:JSONArray,enabled:Boolean,onSelect:()->Unit) {
    val param=desc.getString("id")
    val target=JSONObject().put("kind","effect").put("object",objectId).put("effect",instance).put("param",param)
    val actions=remember(vm,objectId,instance,param){ProjectPositionActions(vm)}
    val dimensions=if(desc.getString("kind")=="vec2")2 else 3
    key(objectId,instance,param){PositionControls(desc.getString("name"),"effect-position-$instance-$param",value,(0 until dimensions).toList(),enabled,desc.getDouble("min"),desc.getDouble("max"),
        onSelect=onSelect,onBegin={actions.begin(target)},onValue={next,axes->actions.value(next,axes)},
        onFinish={actions.finish(it)},inertiaGroup=vm.gestureInertia)}
}

/** Relative XY/Z pad. Hosts provide their own property transaction. */
@Composable internal fun TransformTouchpad(modifier:Modifier,axes:List<Int>,enabled:Boolean,
    lockedAxis:Int?=null,
    onBegin:()->Unit,onDelta:(Double,Double,Boolean)->Unit,onFinish:(Boolean)->Unit,onBack:(()->Unit)?=null,zTag:String="position-pad-z") {
    var z by remember{mutableStateOf(false)}
    val density=LocalDensity.current.density
    val start by rememberUpdatedState(onBegin);val move by rememberUpdatedState(onDelta);val end by rememberUpdatedState(onFinish)
    Box(modifier.background(Background,RoundedCornerShape(10.dp)).pointerInput(enabled,z,density) {
        if(enabled)detectDragGestures(onDragStart={start()},onDragEnd={end(true)},onDragCancel={end(false)}){change,delta->
            change.consume();move(delta.x/density.toDouble(),delta.y/density.toDouble(),z)
        }
    }) {
        Canvas(Modifier.fillMaxSize()) {
            drawLine(Muted.copy(alpha=.12f),Offset(size.width/2,0f),Offset(size.width/2,size.height),1f)
            drawLine(Muted.copy(alpha=.12f),Offset(0f,size.height/2),Offset(size.width,size.height/2),1f)
        }
        Text(if(lockedAxis!=null)if(lockedAxis==0)"左右滑动调整 X"else"上下滑动调整 ${listOf("X","Y","Z")[lockedAxis]}"else if(z)"上下滑动调整 Z"else"滑动移动 · XY",Modifier.align(Alignment.Center),color=Muted,fontSize=12.sp)
        if(onBack!=null)TextButton(onClick=onBack,modifier=Modifier.align(Alignment.TopEnd).heightIn(min=48.dp).testTag("position-pad-back")){Text("返回滑轮")}
        if(lockedAxis==null&&2 in axes)TextButton(onClick={z=!z},modifier=Modifier.align(if(onBack==null)Alignment.TopEnd else Alignment.BottomEnd).size(48.dp).testTag(zTag)){Text(if(z)"XY"else"Z")}
    }
}
