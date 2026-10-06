package com.motionstudio.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.*
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.*
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.semantics.*
import androidx.compose.ui.unit.dp
import kotlin.math.*

/** One 10 dp mark never exceeds 10 units, even for very wide parameter bounds. */
internal fun numericWheelStep(minimum:Double,maximum:Double):Double {
    val desired=((maximum-minimum)/100.0).coerceAtMost(10.0)
    val magnitude=10.0.pow(floor(log10(desired)))
    val ratio=desired/magnitude
    return (if(ratio>=5)5.0 else if(ratio>=2)2.0 else 1.0)*magnitude
}

/** Relative, continuous adjustment: the centre stays fixed as the scale moves. */
@Composable internal fun NumericWheel(
    value:Double,minimum:Double,maximum:Double,label:String,enabled:Boolean,
    modifier:Modifier=Modifier,onValueChange:(Double)->Unit,onFinished:()->Unit,onCancelled:()->Unit,
    inertiaGroup:GestureInertiaGroup?=null,
) {
    val currentValue by rememberUpdatedState(value)
    val change by rememberUpdatedState(onValueChange)
    val finish by rememberUpdatedState(onFinished)
    val cancel by rememberUpdatedState(onCancelled)
    val inertia=rememberGestureInertia(inertiaGroup)
    val limits=rememberFlingLimits()
    var dragging by remember{mutableStateOf(false)}
    DisposableEffect(enabled,minimum,maximum) {
        if(!enabled)inertia.stop()
        onDispose{inertia.stop();if(dragging){dragging=false;cancel()}}
    }
    val step=remember(minimum,maximum){numericWheelStep(minimum,maximum)}
    Canvas(modifier.height(48.dp).clipToBounds().semantics {
        contentDescription="$label，左右滑动调整"
        progressBarRangeInfo=ProgressBarRangeInfo(value.toFloat(),minimum.toFloat()..maximum.toFloat())
        if(enabled)setProgress { next->
            if(!next.isFinite())false else {
                inertiaGroup?.stop();inertia.stop()
                change(next.toDouble().coerceIn(minimum,maximum));finish();true
            }
        }else disabled()
    }.pointerInput(enabled,minimum,maximum) {
        if(!enabled)return@pointerInput
        val spacing=10.dp.toPx()
        awaitEachGesture {
            val down=awaitFirstDown(requireUnconsumed=false)
            inertiaGroup?.stop();inertia.stop()
            val tracker=VelocityTracker().also{it.addPosition(down.uptimeMillis,down.position)}
            var lastMovedAt=down.uptimeMillis
            var lastPointer=down.id
            var dragged=currentValue
            fun advance(delta:Float):Boolean {
                val next=(dragged-delta/spacing*step).coerceIn(minimum,maximum)
                if(next!=dragged){dragged=next;change(next)}
                return next>minimum&&next<maximum
            }
            try {
                val drag=awaitHorizontalTouchSlopOrCancellation(down.id){event,over->
                    tracker.addPosition(event.uptimeMillis,event.position);lastMovedAt=event.uptimeMillis
                    dragging=true;event.consume();advance(over)
                }
                if(drag!=null) {
                    val released=horizontalDrag(drag.id){event->
                        lastPointer=event.id
                        tracker.addPosition(event.uptimeMillis,event.position)
                        val delta=event.positionChange().x
                        if(delta!=0f)lastMovedAt=event.uptimeMillis
                        event.consume();advance(delta)
                    }
                    dragging=false
                    if(released) {
                        val up=currentEvent.changes.firstOrNull{it.id==lastPointer}
                        if(up!=null)tracker.addPosition(up.uptimeMillis,up.position)
                        val velocity=if(up!=null)releaseVelocity(tracker.calculateVelocity().x,up.uptimeMillis,lastMovedAt,limits)else 0f
                        inertia.start(velocity,limits.minimum,2f,::advance){finish()}
                    }else cancel()
                }
            }finally {
                if(dragging){dragging=false;cancel()}
            }
        }
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
