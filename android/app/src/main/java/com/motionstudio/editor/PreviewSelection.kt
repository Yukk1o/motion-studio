package com.motionstudio.editor

import androidx.compose.ui.geometry.Offset
import kotlin.math.*

internal fun previewPolygons(vm:EditorViewModel,width:Float,height:Float):List<Pair<Long,List<Offset>>> {
    val p=vm.state.project?:return emptyList()
    val layers=vm.state.sample?.optJSONArray("projectedLayers")?:return emptyList()
    val fit=min(width/p.getInt("width"),height/p.getInt("height"))
    val left=(width-p.getInt("width")*fit)/2;val top=(height-p.getInt("height")*fit)/2
    return (0 until layers.length()).map{index->
        val layer=layers.getJSONObject(index);val corners=layer.getJSONArray("corners")
        layer.getLong("id") to (0 until corners.length()).map{i->corners.getJSONArray(i).let{Offset(left+it.getDouble(0).toFloat()*fit,top+it.getDouble(1).toFloat()*fit)}}
    }
}
internal fun insideQuad(point:Offset,corners:List<Offset>):Boolean {
    var sign=0
    for(i in corners.indices) {
        val a=corners[i];val b=corners[(i+1)%corners.size]
        val cross=(b.x-a.x)*(point.y-a.y)-(b.y-a.y)*(point.x-a.x)
        if(abs(cross)<.001f)continue
        val next=if(cross>0)1 else -1
        if(sign!=0&&sign!=next)return false
        sign=next
    }
    return sign!=0
}
internal fun previewAnchor(vm:EditorViewModel,objectId:Long,width:Float,height:Float):Offset? {
    val project=vm.state.project?:return null
    val layers=vm.state.sample?.optJSONArray("projectedLayers")?:return null
    val fit=min(width/project.getInt("width"),height/project.getInt("height"))
    val left=(width-project.getInt("width")*fit)/2;val top=(height-project.getInt("height")*fit)/2
    for(i in 0 until layers.length()) {
        val layer=layers.getJSONObject(i)
        if(layer.getLong("id")==objectId)layer.optJSONArray("anchor")?.let{
            if(!it.isNull(0)&&!it.isNull(1))return Offset(left+it.getDouble(0).toFloat()*fit,top+it.getDouble(1).toFloat()*fit)
        }
    }
    return null
}
