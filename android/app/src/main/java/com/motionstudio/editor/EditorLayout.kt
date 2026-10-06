package com.motionstudio.editor

import android.content.SharedPreferences
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt

internal class EditorLayoutState(private val preferences:SharedPreferences) {
    private val ratios=mutableStateMapOf<String,Float>()
    private var before:Map<String,Float>?=null
    init {
        for((key,value) in preferences.all)if(value is Float&&value.isFinite()&&value in .05f.. .85f)ratios[key]=value
    }
    fun value(key:String,extent:Float,default:Float,min:Float,max:Float):Float {
        val upper=max.coerceAtLeast(1f)
        return (ratios[key]?.times(extent)?:default).coerceIn(min.coerceAtMost(upper),upper)
    }
    fun custom(key:String)=key in ratios
    fun set(key:String,value:Float){if(value.isFinite())ratios[key]=value.coerceIn(.05f,.85f)}
    fun begin(){before=ratios.toMap()}
    fun commit(){before=null;val editor=preferences.edit().clear();ratios.forEach{(k,v)->editor.putFloat(k,v)};editor.apply()}
    fun cancel(){before?.let{ratios.clear();ratios.putAll(it)};before=null}
    fun reset(){before=null;ratios.clear();preferences.edit().clear().apply()}
}

/** The touch target stays inside the preview, away from timeline keys and controls. */
@Composable internal fun LayoutGrip(bounds:Rect,origin:Offset,vertical:Boolean,label:String,tag:String,
    layout:EditorLayoutState,onDelta:(Float)->Unit) {
    val density=LocalDensity.current.density
    val width=(if(vertical)48 else 80)*density
    val height=(if(vertical)80 else 48)*density
    val x=if(vertical)bounds.right-width else bounds.center.x-width/2
    val y=if(vertical)bounds.center.y-height/2 else bounds.bottom-height
    val change by rememberUpdatedState(onDelta)
    fun adjust(amount:Float){layout.begin();change(amount);layout.commit()}
    Box(Modifier.offset{IntOffset((x-origin.x).roundToInt(),(y-origin.y).roundToInt())}
        .size((if(vertical)48 else 80).dp,(if(vertical)80 else 48).dp)
        .testTag(tag).semantics {
            contentDescription=label
            customActions=listOf(
                CustomAccessibilityAction("增大预览区域"){adjust(24*density);true},
                CustomAccessibilityAction("减小预览区域"){adjust(-24*density);true},
                CustomAccessibilityAction("恢复默认布局"){layout.reset();true})
        }.pointerInput(vertical,layout) {
            detectDragGestures(onDragStart={layout.begin()},onDragEnd=layout::commit,onDragCancel=layout::cancel) {event,delta->
                event.consume();change(if(vertical)delta.x else delta.y)
            }
        }) {
        Canvas(Modifier.fillMaxSize()) {
            val stroke=3.dp.toPx();val length=28.dp.toPx();val margin=6.dp.toPx()
            val position=if(vertical)Offset(size.width-margin-stroke,size.height/2-length/2)
                else Offset(size.width/2-length/2,size.height-margin-stroke)
            val extent=if(vertical)Size(stroke,length)else Size(length,stroke)
            drawRoundRect(Background.copy(alpha=.85f),position-Offset(3.dp.toPx(),3.dp.toPx()),
                Size(extent.width+6.dp.toPx(),extent.height+6.dp.toPx()),androidx.compose.ui.geometry.CornerRadius(4.dp.toPx()))
            drawRoundRect(Muted,position,extent,androidx.compose.ui.geometry.CornerRadius(stroke/2))
        }
    }
}
