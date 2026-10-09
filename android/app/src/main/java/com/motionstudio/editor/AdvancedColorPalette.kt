package com.motionstudio.editor

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Colorize
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.*

/** Compact native palette in the effect dock. Number entry is a secondary view. */
@Composable internal fun AdvancedColorEditor(
    title:String,initial:Rgba,modifier:Modifier,alphaEditable:Boolean=true,
    range:ClosedFloatingPointRange<Double> = 0.0..1.0,
    onPreview:(Rgba)->Unit,onConfirm:()->Unit,onCancel:()->Unit,onBack:()->Unit,
    onEyedropper:((Rgba)->Unit)->Unit,
) {
    var value by remember{mutableStateOf(initial)}
    val first=remember{initial.hsv()}
    var hue by remember{mutableDoubleStateOf(first[0])}
    var saturation by remember{mutableDoubleStateOf(first[1])}
    var brightness by remember{mutableDoubleStateOf(first[2])}
    var wheel by remember{mutableStateOf(false)}
    var numbers by remember{mutableStateOf(false)}
    var hex by remember{mutableStateOf(initial.hex(alphaEditable))}
    var hexInvalid by remember{mutableStateOf(false)}
    var invalidComponents by remember{mutableStateOf(setOf<Int>())}
    var saving by remember{mutableStateOf(false)}
    val context=LocalContext.current
    val bookmarks=remember(context){ColorBookmarks(context)}
    val focus=LocalFocusManager.current
    val keyboard=LocalSoftwareKeyboardController.current
    val clipboard=LocalClipboardManager.current
    val valid=(0 until if(alphaEditable)4 else 3).all{value.component(it).isFinite()&&value.component(it) in range}
    fun stopTyping(){focus.clearFocus();keyboard?.hide()}
    fun update(next:Rgba,fromHsv:Boolean=false,fromHex:Boolean=false) {
        val changed=next!=value
        value=next
        if(!fromHsv){val hsv=next.hsv(hue);hue=hsv[0];saturation=hsv[1];brightness=hsv[2]}
        if(!fromHex){hex=next.hex(alphaEditable);hexInvalid=false}
        if(changed&&(0 until if(alphaEditable)4 else 3).all{next.component(it) in range})onPreview(next)
    }
    val liveValue by rememberUpdatedState(value)
    val liveHue by rememberUpdatedState(hue)
    val liveSat by rememberUpdatedState(saturation)
    val liveBright by rememberUpdatedState(brightness)
    BoxWithConstraints(modifier.fillMaxSize().testTag("color-palette")) {
    val compact=maxWidth<344.dp
    val narrow=maxWidth<300.dp
    Surface(Modifier.fillMaxSize(),color=Panel,shape=RoundedCornerShape(topStart=16.dp,topEnd=16.dp)) {
        Column(Modifier.padding(horizontal=12.dp)) {
            Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                if(numbers) {
                    IconButton(onClick={stopTyping();hex=value.hex(alphaEditable);hexInvalid=false;invalidComponents=emptySet();numbers=false},modifier=Modifier.size(48.dp).testTag("color-values-back")){Icon(Icons.AutoMirrored.Filled.ArrowBack,"返回色盘")}
                    Text("颜色数值",Modifier.weight(1f),color=Ink,fontSize=14.sp)
                    IconButton(onClick={clipboard.setText(AnnotatedString(value.hex(alphaEditable)))},modifier=Modifier.size(48.dp).testTag("color-copy")){Icon(Icons.Default.ContentCopy,"复制颜色代码")}
                }else {
                    listOf(false to "色板",true to "色环").forEach{(mode,label)->
                        TextButton(onClick={stopTyping();wheel=mode},contentPadding=PaddingValues(horizontal=6.dp),
                            modifier=Modifier.width(52.dp).heightIn(min=48.dp).testTag(if(mode)"color-mode-wheel"else"color-mode-board").semantics{selected=wheel==mode}) {
                            Text(label,color=if(wheel==mode)Ink else Muted,fontSize=14.sp)
                        }
                    }
                    IconButton(onClick={stopTyping();onEyedropper{sample->update(liveValue.copy(r=sample.r,g=sample.g,b=sample.b))}},modifier=Modifier.size(48.dp).testTag("color-eyedropper")){Icon(Icons.Default.Colorize,"吸管")}
                    Surface(onClick={numbers=true},modifier=Modifier.weight(1f).heightIn(min=48.dp).testTag("color-code"),color=Background,shape=RoundedCornerShape(12.dp)) {
                        Row(Modifier.padding(horizontal=8.dp),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(6.dp)) {
                            if(!narrow)Canvas(Modifier.size(24.dp).clip(RoundedCornerShape(6.dp))){drawRect(value.color(alpha=false))}
                            Text(value.hex(false),color=Ink,fontSize=12.sp,maxLines=1)
                        }
                    }
                }
                if(numbers||!compact)IconButton(onClick={saving=true},modifier=Modifier.size(48.dp).testTag("color-palette-favorite")){Icon(Icons.Default.Star,"收藏当前颜色")}
            }
            BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
                val viewport=maxHeight
                val paneWidth=maxWidth
                if(numbers)Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()),verticalArrangement=Arrangement.spacedBy(12.dp)) {
                    Text(title,color=Muted,fontSize=13.sp)
                    OutlinedTextField(hex,{raw->hex=raw;val parsed=Rgba.hex(raw,value.a,alphaEditable);hexInvalid=parsed==null;if(parsed!=null)update(parsed,fromHex=true)},
                        modifier=Modifier.fillMaxWidth().testTag("color-hex"),singleLine=true,isError=hexInvalid,label={Text(if(alphaEditable)"HEX · RGB / RGBA"else"HEX · RGB")})
                    val count=if(alphaEditable)4 else 3
                    val columns=if(paneWidth>=320.dp)count else 2
                    Column(verticalArrangement=Arrangement.spacedBy(8.dp)) {
                        (0 until count).toList().chunked(columns).forEach{row->Row(horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                            row.forEach{index->ColorComponentField(index,value,Modifier.weight(1f),onValidity={ok->invalidComponents=if(ok)invalidComponents-index else invalidComponents+index}){next->update(value.with(index,next))}}
                            repeat(columns-row.size){Spacer(Modifier.weight(1f))}
                        }}
                    }
                    if(!valid)Text("颜色分量范围：${range.start}–${range.endInclusive}",color=MaterialTheme.colorScheme.error,fontSize=12.sp)
                }else if(!wheel) {
                    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                        val boardHeight=(viewport-(if(alphaEditable)96.dp else 48.dp)).coerceIn(64.dp,240.dp)
                        fun choose(p:Offset,w:Int,h:Int){stopTyping();saturation=(p.x/w).toDouble().coerceIn(0.0,1.0);brightness=(1-p.y/h).toDouble().coerceIn(0.0,1.0);update(Rgba.hsv(liveHue,saturation,brightness,liveValue.a),true)}
                        Canvas(Modifier.fillMaxWidth().height(boardHeight).clip(RoundedCornerShape(12.dp)).testTag("color-sv")
                            .pointerInput(Unit){detectTapGestures{choose(it,size.width,size.height)}}
                            .pointerInput(Unit){detectDragGestures(onDragStart={choose(it,size.width,size.height)}){change,_->change.consume();choose(change.position,size.width,size.height)}}) {
                            drawRect(Brush.horizontalGradient(listOf(Color.White,Rgba.hsv(hue,1.0,1.0,1.0).color())))
                            drawRect(Brush.verticalGradient(listOf(Color.Transparent,Color.Black)))
                            val margin=8.dp.toPx()
                            val p=Offset((saturation*size.width).toFloat().coerceIn(margin,size.width-margin),((1-brightness)*size.height).toFloat().coerceIn(margin,size.height-margin))
                            drawCircle(Color.Black.copy(alpha=.3f),8.dp.toPx(),p,style=Stroke(4.dp.toPx()));drawCircle(Color.White,8.dp.toPx(),p,style=Stroke(2.dp.toPx()))
                        }
                        ColorRail("color-hue","色相",hue/360,listOf(Color.Red,Color.Yellow,Color.Green,Color.Cyan,Color.Blue,Color.Magenta,Color.Red),Rgba.hsv(hue,1.0,1.0,1.0).color()){
                            stopTyping();hue=it*360;update(Rgba.hsv(hue,liveSat,liveBright,liveValue.a),true)
                        }
                        if(alphaEditable)ColorRail("color-alpha","不透明度",value.a,listOf(value.color().copy(alpha=0f),value.color(false)),value.color(),checker=true){stopTyping();update(liveValue.copy(a=it))}
                    }
                }else {
                    Row(Modifier.fillMaxSize().verticalScroll(rememberScrollState()),horizontalArrangement=Arrangement.spacedBy(12.dp),verticalAlignment=Alignment.CenterVertically) {
                        val ringSize=(viewport*.72f).coerceIn(84.dp,180.dp).coerceAtMost(paneWidth*.46f)
                        fun choose(p:Offset,w:Int,h:Int){stopTyping();hue=((atan2((p.y-h/2f).toDouble(),(p.x-w/2f).toDouble())*180/PI)+360)%360;update(Rgba.hsv(hue,liveSat,liveBright,liveValue.a),true)}
                        Box(Modifier.size(ringSize),contentAlignment=Alignment.Center) {
                            Canvas(Modifier.fillMaxSize().testTag("color-hue-wheel")
                                .pointerInput(Unit){detectTapGestures{choose(it,size.width,size.height)}}
                                .pointerInput(Unit){detectDragGestures(onDragStart={choose(it,size.width,size.height)}){change,_->change.consume();choose(change.position,size.width,size.height)}}) {
                                val rim=5.dp.toPx();val ring=size.minDimension*.19f;val radius=(size.minDimension-ring)/2-rim
                                drawCircle(Brush.sweepGradient(listOf(Color.Red,Color.Yellow,Color.Green,Color.Cyan,Color.Blue,Color.Magenta,Color.Red)),radius,style=Stroke(ring))
                                val angle=hue*PI/180
                                val p=Offset(size.width/2+(cos(angle)*radius).toFloat(),size.height/2+(sin(angle)*radius).toFloat())
                                drawCircle(Rgba.hsv(hue,1.0,1.0,1.0).color(),ring*.58f,p);drawCircle(Color.White,ring*.58f,p,style=Stroke(2.dp.toPx()))
                            }
                            Text("${hue.roundToInt()}°",color=Muted,fontSize=14.sp)
                        }
                        Column(Modifier.weight(1f)) {
                            ColorRailLabel("饱和度",saturation)
                            ColorRail("color-saturation","饱和度",saturation,listOf(Color.White,Rgba.hsv(hue,1.0,1.0,1.0).color()),Rgba.hsv(hue,saturation,1.0,1.0).color()){
                                stopTyping();saturation=it;update(Rgba.hsv(liveHue,saturation,liveBright,liveValue.a),true)
                            }
                            ColorRailLabel("亮度",brightness)
                            ColorRail("color-brightness","亮度",brightness,listOf(Color.Black,Rgba.hsv(hue,1.0,1.0,1.0).color()),Rgba.hsv(hue,1.0,brightness,1.0).color()){
                                stopTyping();brightness=it;update(Rgba.hsv(liveHue,liveSat,brightness,liveValue.a),true)
                            }
                            if(alphaEditable){ColorRailLabel("不透明度",value.a);ColorRail("color-alpha","不透明度",value.a,listOf(value.color().copy(alpha=0f),value.color(false)),value.color(),checker=true){stopTyping();update(liveValue.copy(a=it))}}
                        }
                    }
                }
            }
            Row(Modifier.fillMaxWidth().heightIn(min=48.dp),verticalAlignment=Alignment.CenterVertically) {
                IconButton(onClick={stopTyping();onBack()},modifier=Modifier.size(48.dp).testTag("color-quick-back")){Icon(Icons.AutoMirrored.Filled.ArrowBack,"返回颜色选择")}
                Text(title,Modifier.weight(1f),color=Muted,fontSize=12.sp,maxLines=1)
                if(compact&&!numbers)IconButton(onClick={saving=true},modifier=Modifier.size(48.dp).testTag("color-palette-favorite")){Icon(Icons.Default.Star,"收藏当前颜色")}
                TextButton(onClick={stopTyping();onCancel()},modifier=Modifier.heightIn(min=48.dp).testTag("color-cancel")){Text("取消")}
                TextButton(onClick={stopTyping();onConfirm()},enabled=valid&&!hexInvalid&&invalidComponents.isEmpty(),modifier=Modifier.heightIn(min=48.dp).testTag("color-confirm")){Text("完成")}
            }
        }
    }
    }
    if(saving)InputDialog("收藏颜色",value.hex(),{saving=false}){name->bookmarks.add(name,value);saving=false}
}

@Composable private fun ColorRailLabel(label:String,value:Double) {
    Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically){Text(label,Modifier.weight(1f),color=Muted,fontSize=12.sp,lineHeight=14.sp);Text("${(value*100).roundToInt()}",color=Muted,fontSize=12.sp,lineHeight=14.sp)}
}

@Composable private fun ColorRail(tag:String,label:String,value:Double,colors:List<Color>,thumb:Color,checker:Boolean=false,onChange:(Double)->Unit) {
    val liveChange by rememberUpdatedState(onChange)
    Canvas(Modifier.fillMaxWidth().height(48.dp).testTag(tag)
        .semantics{contentDescription=label;progressBarRangeInfo=ProgressBarRangeInfo(value.toFloat(),0f..1f);setProgress{liveChange(it.toDouble().coerceIn(0.0,1.0));true}}
        .pointerInput(Unit){fun choose(p:Offset){val margin=10.dp.toPx();liveChange(((p.x-margin)/(size.width-2*margin)).toDouble().coerceIn(0.0,1.0))};detectTapGestures{choose(it)}}
        .pointerInput(Unit){fun choose(p:Offset){val margin=10.dp.toPx();liveChange(((p.x-margin)/(size.width-2*margin)).toDouble().coerceIn(0.0,1.0))};detectDragGestures(onDragStart={choose(it)}){change,_->change.consume();choose(change.position)}}) {
        val margin=10.dp.toPx();val height=if(checker)20.dp.toPx()else 16.dp.toPx();val top=(size.height-height)/2
        val rect=Rect(margin,top,size.width-margin,top+height)
        val clip=Path().apply{addRoundRect(RoundRect(rect,CornerRadius(height/2)))}
        clipPath(clip){if(checker)checkerboard(6.dp.toPx());drawRect(Brush.horizontalGradient(colors,startX=margin,endX=size.width-margin),rect.topLeft,rect.size)}
        val p=Offset(margin+(size.width-2*margin)*value.toFloat(),size.height/2)
        drawCircle(Background,10.dp.toPx(),p);drawCircle(thumb,8.dp.toPx(),p);drawCircle(Color.White,9.dp.toPx(),p,style=Stroke(2.dp.toPx()))
    }
}
