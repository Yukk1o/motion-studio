(function(){
    var root=File($.fileName).parent.parent;var folder=new Folder(root.fsName+"/artifacts/ae-tiling/18.0.1");
    function read(name){var f=new File(folder.fsName+"/"+name);f.encoding="UTF-8";f.open("r");var text=f.read();f.close();return eval("("+text+")");}
    function quote(v){return '"'+String(v).replace(/\\/g,"\\\\").replace(/"/g,'\\"').replace(/\n/g,"\\n").replace(/\r/g,"\\r")+'"';}
    function write(name,text){var f=new File(folder.fsName+"/"+name);f.encoding="UTF-8";f.open("w");f.write(text);f.close();}
    try {
        app.beginSuppressDialogs();app.newProject();app.project.bitsPerChannel=8;app.project.workingSpace="sRGB IEC61966-2.1";app.project.linearizeWorkingSpace=false;
        var cases=read("cases.json"),footage={},outputs=[];
        for(var i=0;i<cases.length;i++){
            var c=cases[i];if(!footage[c.input]){var f=app.project.importFile(new ImportOptions(new File(folder.fsName+"/"+c.input)));f.mainSource.alphaMode=AlphaMode.STRAIGHT;footage[c.input]=f;}
            var comp=app.project.items.addComp(c.id,256,256,1,1/30,30);comp.motionBlur=false;var layer=comp.layers.add(footage[c.input]);
            var fx=layer.property("ADBE Effect Parade").addProperty(c.matchName);for(var key in c.properties)if(c.properties.hasOwnProperty(key))fx.property(key).setValue(c.properties[key]);
            var rq=app.project.renderQueue.items.add(comp);rq.timeSpanStart=0;rq.timeSpanDuration=1/30;
            var om=rq.outputModule(1),templates=om.templates,selected=null;
            for(var j=0;j<templates.length;j++)if(/TIFF/i.test(templates[j])&&/Alpha/i.test(templates[j])){selected=templates[j];break;}
            if(!selected)throw Error("A TIFF+Alpha output template is required");
            om.applyTemplate(selected);
            // Frame tokens are mandatory: AE otherwise produces empty placeholder files.
            om.file=new File(folder.fsName+"/ae-cli-"+c.id+"-[#####].tif");
            outputs.push('{"id":'+quote(c.id)+',"queueIndex":'+(i+1)+'}');
        }
        app.project.save(new File(folder.fsName+"/render-references.aep"));

        write("queue.json",'{"version":'+quote(app.version)+',"outputs":['+outputs.join(',')+']}');
        write("render-progress.json",'{"state":"queue_ready","cases":'+cases.length+'}');app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);app.endSuppressDialogs(false);app.exitCode=0;
    }catch(e){write("render-progress.json",'{"state":"failed","error":'+quote(e)+',"line":'+e.line+'}');app.exitCode=1;}
    app.exitAfterLaunchAndEval=true;
}());
