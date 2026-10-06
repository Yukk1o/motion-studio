package com.motionstudio.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import kotlin.math.*

/** Relative, continuous adjustment: the centre stays fixed as the scale moves. */
@Composable internal fun NumericWheel(
    value:Double,minimum:Double,maximum:Double,label:String,enabled:Boolean,
    modifier:Modifier=Modifier,onValueChange:(Double)->Unit,onFinished:()->Unit,onCancelled:()->Unit,
) {
    val currentValue by rememberUpdatedState(value)
    val change by rememberUpdatedState(onValueChange)
    val finish by rememberUpdatedState(onFinished)
    val cancel by rememberUpdatedState(onCancelled)
    val step=remember(minimum,maximum) {
        val desired=(maximum-minimum)/100.0
        val magnitude=10.0.pow(floor(log10(desired)))
        val ratio=desired/magnitude
        (if(ratio>=5)5.0 else if(ratio>=2)2.0 else 1.0)*magnitude
    }
    Canvas(modifier.height(48.dp).clipToBounds().semantics {
        contentDescription="$label，左右滑动调整"
        progressBarRangeInfo=ProgressBarRangeInfo(value.toFloat(),minimum.toFloat()..maximum.toFloat())
        if(enabled)setProgress { next->
            if(!next.isFinite())false else {
                change(next.toDouble().coerceIn(minimum,maximum));finish();true
            }
        }else disabled()
    }.pointerInput(enabled,minimum,maximum) {
        if(!enabled)return@pointerInput
        var dragged=value
        val spacing=10.dp.toPx()
        detectHorizontalDragGestures(
            onDragStart={dragged=currentValue},onDragEnd={finish()},onDragCancel={cancel()},
            onHorizontalDrag={event,delta->
                event.consume()
                dragged=(dragged-delta/spacing*step).coerceIn(minimum,maximum)
                change(dragged)
            },
        )
    }) {
        val spacing=10.dp.toPx()
        val position=value/step
        val first=floor(position)
        val count=ceil(size.width/spacing/2).toInt()+1
        for(i in -count..count) {
            val mark=first+i
            val tickValue=mark*step
            if(tickValue<minimum-1e-9||tickValue>maximum+1e-9)continue
            val x=size.width/2+((mark-position)*spacing).toFloat()
            val length=if(abs(mark%5)<1e-6)22.dp.toPx()else 12.dp.toPx()
            drawLine(Muted.copy(alpha=if(enabled).55f else .2f),Offset(x,(size.height-length)/2),Offset(x,(size.height+length)/2),1.dp.toPx())
        }
        drawLine(if(enabled)Accent else Muted,Offset(size.width/2,size.height/2-14.dp.toPx()),Offset(size.width/2,size.height/2+14.dp.toPx()),2.dp.toPx())
    }
}
