#!/usr/bin/env python3
"""Read existing Kubernetes node and PVC statistics before a Sprøyt promotion."""
import argparse
import json
import re
import subprocess
import sys
from datetime import datetime, timezone

GIB = 1024**3
MIB = 1024**2

def quantity(value, units):
    match = re.fullmatch(r'(\d+)([A-Za-z]*)', value)
    if not match or match[2] not in units:
        raise ValueError(f'unrecognised Kubernetes quantity {value!r}')
    return int(match[1]) * units[match[2]]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--context', required=True)
    args = parser.parse_args()
    def kube(*command):
        return subprocess.check_output(['kubectl', '--context', args.context, *command], text=True)
    nodes = json.loads(kube('get', 'nodes', '-o', 'json'))['items']
    problems = []
    volumes = []
    checked = 0
    for node in nodes:
        name = node['metadata']['name']
        if node['spec'].get('unschedulable'):
            continue
        checked += 1
        states = {c['type']: c['status'] for c in node['status']['conditions']}
        if states.get('Ready') != 'True' or any(states.get(k) != 'False' for k in ('MemoryPressure','DiskPressure','PIDPressure')):
            problems.append(f'{name}: readiness or pressure')
        stats = json.loads(kube('get', '--raw', f'/api/v1/nodes/{name}/proxy/stats/summary'))
        for resource in ('cpu', 'memory', 'fs'):
            at = datetime.fromisoformat(stats['node'][resource]['time'].replace('Z','+00:00'))
            if not 0 <= (datetime.now(timezone.utc)-at).total_seconds() <= 180:
                problems.append(f'{name}: stale {resource} sample')
        alloc = node['status']['allocatable']
        described = kube('describe', 'node', name).split('Allocated resources:', 1)[1]
        def free(resource, used, units):
            request = re.search(r'^\s+'+resource+r'\s+(\S+)\s+\(', described, re.M)[1]
            return quantity(alloc[resource], units) - max(used, quantity(request, units))
        cpu = free('cpu', stats['node']['cpu']['usageNanoCores']/1e6, {'':1000,'m':1})
        memory = free('memory', stats['node']['memory']['workingSetBytes'],
                      {'':1,'Ki':1024,'Mi':MIB,'Gi':GIB})
        disk = stats['node']['fs']['availableBytes']
        print(f'{name}: {cpu:.0f}m CPU, {memory/MIB:.0f} MiB memory, {disk/GIB:.1f} GiB filesystem available')
        # One whole local release phase plus the per-node operating reserve.
        if cpu < 1500 or memory < 2*GIB or disk < 14*GIB:
            problems.append(f'{name}: phase plus reserve does not fit')
        for pod in stats['pods']:
            for volume in pod.get('volume', []):
                if volume.get('pvcRef') == {'name':'data-postgres-postgresql-0','namespace':'database'}:
                    volumes.append(volume['availableBytes'])
    if checked != 3:
        problems.append('expected three schedulable nodes; review a changed topology')
    if len(volumes) != 1 or volumes[0] < 2*GIB:
        problems.append('missing shared database PVC or less than 2 GiB free')
    else:
        print(f'Shared database PVC: {volumes[0]/GIB:.2f} GiB available')
    if problems:
        sys.exit('HOLD: '+'; '.join(problems))

if __name__ == '__main__':
    main()
