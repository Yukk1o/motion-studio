package com.motionstudio.editor

import androidx.compose.foundation.*
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Colorize
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.json.JSONArray
import java.util.Locale
import kotlin.math.*

internal data class Rgba(val r:Double,val g:Double,val b:Double,val a:Double) {
    fun array()=JSONArray(listOf(r,g,b,a))
    fun color(alpha:Boolean=true)=Color(r.toFloat(),g.toFloat(),b.toFloat(),if(alpha)a.toFloat() else 1f)
    fun component(i:Int)=listOf(r,g,b,a)[i]
    fun with(i:Int,v:Double)=when(i){0->copy(r=v);1->copy(g=v);2->copy(b=v);else->copy(a=v)}
    fun hex(alpha:Boolean=true):String {
        fun byte(v:Double)=(v*255).roundToInt().coerceIn(0,255)
        val rgb=String.format(Locale.US,"#%02X%02X%02X",byte(r),byte(g),byte(b))
        return if(alpha&&a!=1.0)rgb+String.format(Locale.US,"%02X",byte(a))else rgb
    }
    fun hsv(fallbackHue:Double=0.0):DoubleArray {
        val max=maxOf(r,g,b);val min=minOf(r,g,b);val delta=max-min
        val h=if(delta<1e-12)fallbackHue else ((when(max){r->(g-b)/delta;g->(b-r)/delta+2;else->(r-g)/delta+4})*60+360)%360
        return doubleArrayOf(h,if(max==0.0)0.0 else delta/max,max)
    }
    companion object {
        fun from(v:JSONArray)=Rgba(v.getDouble(0),v.getDouble(1),v.getDouble(2),v.getDouble(3))
        fun hex(raw:String,alpha:Double,allowAlpha:Boolean):Rgba? {
            val s=raw.trim().removePrefix("#")
            if(s.length !in listOf(6,8)||!s.all{it.isDigit()||it.lowercaseChar() in 'a'..'f'}||(!allowAlpha&&s.length==8))return null
            val n=s.toLongOrNull(16)?:return null
            return if(s.length==6)Rgba((n shr 16 and 255)/255.0,(n shr 8 and 255)/255.0,(n and 255)/255.0,alpha)
                else Rgba((n shr 24 and 255)/255.0,(n shr 16 and 255)/255.0,(n shr 8 and 255)/255.0,(n and 255)/255.0)
        }
        fun hsv(hue:Double,s:Double,v:Double,alpha:Double):Rgba {
            val h=((hue%360)+360)%360/60;val c=v*s;val x=c*(1-abs(h%2-1));val m=v-c
            val rgb=when(h.toInt()){0->doubleArrayOf(c,x,0.0);1->doubleArrayOf(x,c,0.0);2->doubleArrayOf(0.0,c,x);3->doubleArrayOf(0.0,x,c);4->doubleArrayOf(x,0.0,c);else->doubleArrayOf(c,0.0,x)}
            return Rgba(rgb[0]+m,rgb[1]+m,rgb[2]+m,alpha)
        }
    }
}

@Composable internal fun SourceColorProperty(vm:EditorViewModel) {
    val objectId=vm.selected
    val content=vm.layer(objectId)?.optJSONObject("content")?:return
    if(content.optString("kind") !in listOf("solid","text"))return
    val color=content.optJSONArray("color")?:return
    ColorProperty(vm,"颜色","source-color",color,vm.editable(),docked=false){rgba,_->vm.edit(org.json.JSONObject().put("op","set_color").put("object",objectId).put("value",rgba),false)}
}
@Composable internal fun BackgroundColorProperty(vm:EditorViewModel) {
    val color=vm.state.project?.optJSONArray("background")?:return
    ColorProperty(vm,"背景颜色","background-color",color){rgba,_->vm.edit(org.json.JSONObject().put("op","set_color").put("object",0).put("value",rgba),false)}
}

internal fun androidx.compose.ui.graphics.drawscope.DrawScope.checkerboard(cell:Float=8.dp.toPx()) {
    for(y in 0..ceil(size.height/cell).toInt())for(x in 0..ceil(size.width/cell).toInt())
        drawRect(if((x+y)%2==0)Color(0xFF34404E)else Color(0xFF657181),Offset(x*cell,y*cell),androidx.compose.ui.geometry.Size(cell,cell))
}

@Composable internal fun ColorComponentField(index:Int,value:Rgba,modifier:Modifier,onValidity:(Boolean)->Unit,onChange:(Double)->Unit) {
    val factor=if(index==3)100.0 else 255.0
    var text by remember{mutableStateOf(String.format(Locale.US,"%.2f",value.component(index)*factor).trimEnd('0').trimEnd('.'))}
    var editing by remember{mutableStateOf(false)}
    LaunchedEffect(value.component(index)){if(!editing){text=String.format(Locale.US,"%.2f",value.component(index)*factor).trimEnd('0').trimEnd('.');onValidity(true)}}
    val number=text.toDoubleOrNull();val invalid=number==null||!number.isFinite()||number !in 0.0..factor
    OutlinedTextField(text,{raw->text=raw;val n=raw.toDoubleOrNull()?.takeIf{it.isFinite()&&it in 0.0..factor};onValidity(n!=null);n?.let{onChange(it/factor)}},
        modifier=modifier.testTag("color-component-$index").then(Modifier.onFocusChanged{editing=it.isFocused}),singleLine=true,isError=invalid,label={Text(listOf("R","G","B","A %")[index])},keyboardOptions=KeyboardOptions(keyboardType=KeyboardType.Decimal))
}

/** Literal color row: four numeric channel rows are presented as one swatch. */
@Composable internal fun ColorProperty(vm:EditorViewModel,label:String,tag:String,value:JSONArray,enabled:Boolean=true,alphaEditable:Boolean=true,
    range:ClosedFloatingPointRange<Double> = 0.0..1.0,docked:Boolean=true,onSelect:()->Unit={},onSet:(JSONArray,Int)->Unit) {
    val rgba=Rgba.from(value)
    val context=LocalContext.current
    val bookmarks=remember(context){ColorBookmarks(context)}
    var favoriteMenu by remember{mutableStateOf(false)}
    fun open(advanced:Boolean=false):ColorEditingSession {
        vm.pause();onSelect();val session=ColorEditingSession(label,tag,rgba,vm.root,vm.compositionId,floor(vm.frame).toInt(),alphaEditable,range,onSet,docked,vm.panelOpen)
        session.advanced=advanced;vm.openColorEditor(session);return session
    }
    Row(Modifier.fillMaxWidth().heightIn(min=56.dp).horizontalScroll(rememberScrollState()),verticalAlignment=Alignment.CenterVertically) {
        TextButton(onClick={open()},enabled=enabled,modifier=Modifier.heightIn(min=48.dp).testTag(tag)){
            Canvas(Modifier.size(24.dp)){checkerboard();drawRect(rgba.color())}
            Spacer(Modifier.width(8.dp));Text(label,color=Ink,fontSize=14.sp)
        }
        IconButton(onClick={val session=open();vm.beginEyedropper{sample->if(sample!=null&&vm.colorEditor===session)vm.previewColor(session.value.copy(r=sample.r,g=sample.g,b=sample.b))}},enabled=enabled,modifier=Modifier.size(48.dp).testTag("$tag-eyedropper")){Icon(Icons.Default.Colorize,"吸管")}
        IconButton(onClick={open(true)},enabled=enabled,modifier=Modifier.size(48.dp).testTag("$tag-palette")){Icon(Icons.Default.Palette,"调色盘")}
        Box {
            IconButton(onClick={favoriteMenu=true},enabled=enabled,modifier=Modifier.size(48.dp).testTag("$tag-favorites")){Icon(Icons.Default.Star,"收藏")}
            DropdownMenu(favoriteMenu,{favoriteMenu=false}) {
                DropdownMenuItem(text={Text("收藏当前颜色")},onClick={bookmarks.add(rgba.hex(),rgba);favoriteMenu=false})
                bookmarks.list().forEach{entry->DropdownMenuItem(text={Text(entry.name)},enabled=(0 until if(alphaEditable)4 else 3).all{entry.color.component(it) in range},onClick={
                    favoriteMenu=false;vm.pause();onSelect();vm.beginGesture();onSet((if(alphaEditable)entry.color else entry.color.copy(a=rgba.a)).array(),floor(vm.frame).toInt());vm.endGesture()
                })}
            }
        }
        bookmarks.common().forEach{(id,c)->PaletteSwatch(c,c.hex(),"$tag-common-${id.replace(':','-')}",rgba.copy(a=1.0)==c.copy(a=1.0),enabled&&(0 until if(alphaEditable)4 else 3).all{c.component(it) in range}){
            val chosen=if(id.startsWith("favorite:")&&alphaEditable)c else c.copy(a=rgba.a)
            vm.pause();onSelect();vm.beginGesture();onSet(chosen.array(),floor(vm.frame).toInt());vm.endGesture()
        }}
    }
}

internal class ColorEditingSession(val title:String,val tag:String,val original:Rgba,val root:java.io.File,val composition:String,val frame:Int,
    val alphaEditable:Boolean,val range:ClosedFloatingPointRange<Double>,val onSet:(JSONArray,Int)->Unit,val docked:Boolean=true,val previousPanelOpen:Boolean=true) {
    var value by mutableStateOf(original)
    var advanced by mutableStateOf(false)
    var started=false
}
