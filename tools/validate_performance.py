"""Record a fixed workload using a signed, non-debuggable benchmark APK.

Instrumentation returns only its own compressed timing report; no run-as or
debuggable permission is needed. SurfaceFlinger traces remain a separate gate.
"""
import argparse
import base64
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[1]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--serial',required=True)
    parser.add_argument('--seconds',type=int,default=600)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if not 1<=args.seconds<=600: raise ValueError('Duration must be 1..600 seconds')
    shared=next(p for p in [ROOT,*ROOT.parents] if (p/'.tools/environment.json').exists())
    config=json.loads((shared/'.tools/environment.json').read_text())
    adb=[str(Path(config['sdk'])/'platform-tools/adb.exe'),'-s',args.serial]
    args.output.mkdir(parents=True,exist_ok=True)
    provenance={'sourceCommit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'sourceDirty':bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip()),'apks':{}}
    for apk in [ROOT/'android/app/build/outputs/apk/benchmark/app-benchmark.apk',ROOT/'android/app/build/outputs/apk/androidTest/benchmark/app-benchmark-androidTest.apk']:
        provenance['apks'][apk.name]=hashlib.sha256(apk.read_bytes()).hexdigest()
        result=subprocess.run([*adb,'install','-r',str(apk)],capture_output=True,check=True)
        if b'Success' not in result.stdout: raise RuntimeError(result.stdout.decode(errors='replace'))
    (args.output/'provenance.json').write_text(json.dumps(provenance,indent=2),encoding='utf-8')
    try:
        with (args.output/'instrumentation.txt').open('wb') as stream:
            result=subprocess.run([*adb,'shell','am','instrument','-w','-e','class',
                'com.motionstudio.editor.PreviewPerformanceTest#fixedTwentyLayerWorkloadRecordsCpuGpuAndMemoryWithoutFrameImageReadbacks',
                '-e','durationSeconds',str(args.seconds),'-e','requireNonDebuggable','true',
                'com.motionstudio.editor.test/androidx.test.runner.AndroidJUnitRunner'],stdout=stream,stderr=subprocess.STDOUT,timeout=args.seconds+180)
        log=(args.output/'instrumentation.txt').read_text(encoding='utf-8',errors='replace')
        if result.returncode!=0 or 'OK (1 test)' not in log or 'FAILURES!!!' in log: raise RuntimeError('Performance run failed; see instrumentation.txt')
        chunks=re.findall(r'INSTRUMENTATION_STATUS: perfChunk=([A-Za-z0-9+/=]+)',log)
        if not chunks: raise RuntimeError('The signed test did not return a timing report')
        report=json.loads(gzip.decompress(base64.b64decode(''.join(chunks))))
        if report['android']['debuggable']: raise RuntimeError('Debuggable runs are not performance evidence')
        (args.output/'preview-performance.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
        summary={k:v for k,v in report.items() if k!='frames'}
        summary['fullA12AcceptanceComplete']=False
        summary['missingGates']=['Two physical devices','SurfaceFlinger presentation deadlines','Interaction presentation latency','Controlled environment temperature']
        graphics=report['metadata']['graphics']
        pss=max(sample['pssKiB'] for sample in report['android']['memoryAndThermal'])
        cpu=report['cpuPrepareUs']['p95']
        gpu=report['gpuTotalUs']['p95'] if report['gpuTotalUs'] and report['gpuSampleCoverage']>=0.99 else None
        summary['measuredBudgetChecks']={'cpuPrepareP95AtMost4ms':cpu<=4000,'gpuTotalP95AtMost10ms':gpu<=10000 if gpu is not None else None,
            'pssAtMost400MiB':pss<=400*1024,'assetTexturesAtMost128MiB':graphics['assetTextureBytes']<=128*1024*1024,
            'referenceFillAtMostFourScreens':report['workload']['maximumClippedBoundingBoxFillScreens']<=4}
        if gpu is None:summary['missingGates'].append('Reliable GPU timestamp coverage')
        if args.seconds<600:summary['missingGates'].append('Ten-minute duration')
        (args.output/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
        print(json.dumps({'frameCount':summary['frameCount'],'gpuSampleCoverage':summary['gpuSampleCoverage'],'cpuPrepareUs':summary['cpuPrepareUs'],
            'gpuTotalUs':summary['gpuTotalUs'],'maxPssKiB':pss,'displayRefreshHz':report['android']['displayRefreshHz'],'measuredBudgetChecks':summary['measuredBudgetChecks']},indent=2),flush=True)
    finally:
        subprocess.run([*adb,'shell','am','start','-n','com.motionstudio.editor/.MainActivity'],capture_output=True)


if __name__=='__main__':main()
