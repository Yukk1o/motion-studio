// Host mocks verify our collector's traversal and read-only behavior.
// They cannot validate AE-version/plugin-specific API behavior.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
let written, closed = false, preExpressionReads = 0;
class File {
  constructor(name) { this.fsName = name.replaceAll('\\', '/'); this.parent = {fsName:path.posix.dirname(this.fsName)}; }
  open(mode) { assert.equal(mode, 'w'); assert.equal(this.fsName, '/reports/ae-project-export.json'); return true; }
  write(text) { written = JSON.parse(text); }
  close() {}
}
class Folder { constructor(name) { this.fsName = name; this.exists = true; } }
class CompItem {}
class FootageItem {}
class TextLayer {}
class ShapeLayer {}
class CameraLayer {}
class LightLayer {}
class Shape {}
class TextDocument {}
class MarkerValue {}
const PropertyType = {PROPERTY:1, NAMED_GROUP:2};
const PropertyValueType = {NO_VALUE:0, CUSTOM_VALUE:1, OneD:2, LAYER_INDEX:3};
const property = {
  name:'Amount', matchName:'ADBE Test-0001', propertyIndex:1,
  propertyType:1, propertyValueType:2, numKeys:1, canSetExpression:true,
  expression:'evil(); textIndex + effect("control")(1)', expressionEnabled:true,
  hasMin:true, hasMax:true, minValue:0, maxValue:100,
  get value() { throw Error('post-expression evaluation is forbidden'); },
  valueAtTime(time, preExpression) { assert.equal(preExpression, true); preExpressionReads++; return 25; },
  keyTime() { return 0.125; }, keyValue() { return 30; },
  keyInInterpolationType() { return 'BEZIER'; }, keyOutInterpolationType() { return 'HOLD'; },
  keyInTemporalEase() { return [{speed:2,influence:33}]; }, keyOutTemporalEase() { return [{speed:0,influence:50}]; },
};
const effect = {name:'Test', matchName:'ADBE Test', propertyIndex:1,
  propertyType:2, numProperties:1, enabled:false, property() { return property; }};
const child = Object.assign(new CompItem(), {id:2,name:'Child',numLayers:0,frameRate:24,duration:1});
const layer = {index:1,name:'Reference',enabled:true,source:child,parent:null,numProperties:1,
  inPoint:0,outPoint:1,startTime:0,stretch:100,blendingMode:'BlendingMode.ADD',property() { return effect; }};
const root = Object.assign(new CompItem(), {id:1,name:'Root',numLayers:1,frameRate:24,
  layer() { return layer; }});
const project = {file:new File('/source/project.aep'),numItems:2,item(i) { return [root,child][i-1]; },
  close() { closed = true; }};
const app = {version:'mock',effects:[],project};
const context = {app,File,Folder,CompItem,FootageItem,TextLayer,ShapeLayer,CameraLayer,LightLayer,
  Shape,TextDocument,MarkerValue,PropertyType,PropertyValueType,
  CloseOptions:{DO_NOT_SAVE_CHANGES:0}};
vm.createContext(context);
vm.runInContext(fs.readFileSync(path.join(__dirname,'ae_export_project.jsx'), 'utf8'), context);
context.motionStudioExportProject({output:'/reports',rootName:'Root'});
assert.equal(written.state, 'collected');
assert.deepEqual(written.reachableCompositionIds, ['1','2']);
assert.equal(written.compositions[0].layers[0].blendingMode, 'BlendingMode.ADD');
const exported = written.compositions[0].layers[0].properties[0];
assert.equal(exported.enabled, false);
assert.equal(exported.children[0].value, 25);
assert.equal(exported.children[0].keys[0].time, 0.125);
assert.equal(exported.children[0].keys[0].inEase[0].influence, 33);
assert.equal(exported.children[0].expression, property.expression);
assert.equal(preExpressionReads, 1);
assert.equal(closed, false, 'existing project must stay open');
assert.throws(() => context.motionStudioExportProject({output:'/source/reports'}), /outside/);
context.motionStudioExportProject({project:'/source/another.aep',output:'/reports'});
assert.equal(written.state, 'failed');
assert.match(written.errors[0].message, /refusing to replace/);
console.log('AE collector mock checks passed (not an AE integration test).');
