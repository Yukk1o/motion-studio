"""Read actual presentation timestamps for our native SurfaceView only.

SurfaceFlinger rolling timestamps establish cadence. They do not identify the
cause of a missed app deadline; do not label these gaps as app-caused jank.
"""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess
import time

ROOT=Path(__file__).resolve().parents[1]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--serial',required=True)
    parser.add_argument('--seconds',type=int,default=30)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if not 1<=args.seconds<=660:raise ValueError('Duration must be 1..660 seconds')
    shared=next(p for p in [ROOT,*ROOT.parents] if (p/'.tools/environment.json').exists())
    config=json.loads((shared/'.tools/environment.json').read_text())
    command=[str(Path(config['sdk'])/'platform-tools/adb.exe'),'-s',args.serial,'shell']
    args.output.mkdir(parents=True,exist_ok=True)
    records={};periods=set();layers=set();errors=[];started=time.monotonic()
    while time.monotonic()-started<args.seconds:
        try:
            listing=subprocess.check_output([*command,'dumpsys','SurfaceFlinger','--list'],timeout=10).decode()
            names=[]
            for line in listing.splitlines():
                if 'SurfaceView[com.motionstudio.editor/' in line and '(BLAST)' in line:
                    name=line.strip()
                    if name.startswith('RequestedLayerState{'):name=name[len('RequestedLayerState{'):].split(' parentId=')[0]
                    names.append(name)
            for name in names:
                raw=subprocess.check_output([*command,'dumpsys','SurfaceFlinger','--latency',shlex.quote(name)],timeout=10).decode()
                lines=[line.strip() for line in raw.splitlines() if line.strip()]
                if not lines:continue
                period=int(lines[0]);periods.add(period);layers.add(name)
                for line in lines[1:]:
                    if not re.fullmatch(r'\d+\s+\d+\s+\d+',line):continue
                    desired,actual,ready=map(int,line.split())
                    if actual<=0 or actual>=2**63-1:continue
                    records[(name,actual)]={'layer':name,'desiredPresentNs':desired,'actualPresentNs':actual,'frameReadyNs':ready}
        except (subprocess.SubprocessError,ValueError) as e:errors.append(str(e))
        time.sleep(1)
    ordered=sorted(records.values(),key=lambda r:r['actualPresentNs'])
    gaps=[]
    for name in layers:
        points=sorted(r['actualPresentNs'] for r in ordered if r['layer']==name)
        gaps.extend(b-a for a,b in zip(points,points[1:]))
    gaps.sort();period=next(iter(periods)) if len(periods)==1 else None
    summary={'serial':args.serial,'durationSeconds':time.monotonic()-started,'layers':sorted(layers),'records':len(ordered),
        'refreshPeriodsNs':sorted(periods),'presentationIntervalNs':{f'p{p}':gaps[max(0,(len(gaps)*p+99)//100-1)] for p in [50,95,99]} if gaps else None,
        'cadenceGapsAbove1_5Periods':sum(g>period*1.5 for g in gaps) if period else None,'errors':errors,
        'scope':'Actual SurfaceView presentation cadence; does not prove app-caused deadline misses or input-to-display latency.'}
    (args.output/'surface-timestamps.json').write_text(json.dumps(ordered,indent=2),encoding='utf-8')
    (args.output/'summary.json').write_text(json.dumps(summary,indent=2),encoding='utf-8')
    print(json.dumps(summary,indent=2),flush=True)
    if not ordered:raise RuntimeError('No actual native SurfaceView presentation timestamps were available')


if __name__=='__main__':main()
