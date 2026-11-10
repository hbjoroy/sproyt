#!/usr/bin/env python3
"""Validate two rendered Sprøyt releases against their shared Heart role budget."""
import argparse
import json
from pathlib import Path
import yaml

COMPONENT = 'app.kubernetes.io/component'


def env_value(container, key):
    rows = [row for row in container.get('env', []) if row['name'] == key]
    if len(rows) != 1 or 'value' not in rows[0]:
        raise ValueError(f'{container["name"]}: requires one literal {key}')
    return int(rows[0]["value"])


def matches(selector, labels):
    return all(labels.get(k) == v for k, v in selector.items())


def release_budget(documents):
    heart = [d for d in documents if d and d.get('metadata', {}).get('labels', {}).get('app.kubernetes.io/name') == 'heart']
    def one(kind):
        rows = [d for d in heart if d['kind'] == kind]
        if len(rows) != 1:
            raise ValueError(f'requires one Heart {kind}')
        return rows[0]
    deployment, service, pdb = one('Deployment'), one('Service'), one('PodDisruptionBudget')
    spec = deployment['spec']
    if spec['strategy'] != {'type': 'Recreate'}:
        raise ValueError('Heart must wait for old runtime pods using Recreate')
    selector = spec['selector']['matchLabels']
    # This selector existed before the change and is immutable on live Deployments.
    if set(selector) != {'app.kubernetes.io/name', 'app.kubernetes.io/instance'}:
        raise ValueError('immutable Deployment selector must remain unchanged')
    pod = spec['template']
    if pod['metadata']['labels'].get(COMPONENT) != 'runtime' or not matches(selector, pod['metadata']['labels']):
        raise ValueError('runtime pod labels do not match Deployment')
    runtime = pod['spec']['containers']
    if len(runtime) != 1 or pod['spec'].get('initContainers'):
        raise ValueError('unexpected runtime pool consumer')
    replicas = spec['replicas']
    pool = env_value(runtime[0], 'HEART_DB_MAX_CONNECTIONS')
    if not 1 <= replicas <= 10 or not 1 <= pool <= 10:
        raise ValueError('runtime replica/pool bounds must be explicit')
    endpoints = [service['spec']['selector'], pdb['spec']['selector']['matchLabels']]
    for endpoint in endpoints:
        if endpoint.get(COMPONENT) != 'runtime' or not matches(endpoint, pod['metadata']['labels']):
            raise ValueError('Service/PDB must select the runtime component')
    jobs = [d for d in heart if d['kind'] == 'Job']
    if len(jobs) != 2:
        raise ValueError('requires migration and definition jobs')
    weights = []
    commands = []
    for job in jobs:
        js = job['spec']; jp = js['template']['spec']; labels = js['template']['metadata']['labels']
        annotations = job['metadata']['annotations']
        weights.append(int(annotations['helm.sh/hook-weight']))
        if annotations['helm.sh/hook'] != 'pre-install,pre-upgrade' or annotations['helm.sh/hook-delete-policy'] != 'before-hook-creation,hook-succeeded':
            raise ValueError('maintenance must keep ordered, replace-before-create Helm hooks')
        if js.get('parallelism') != 1 or js.get('completions') != 1 or js.get('podReplacementPolicy') != 'Failed':
            raise ValueError('maintenance retry must wait for fully terminated predecessor')
        if not 1 <= js.get('activeDeadlineSeconds', 0) <= 180 or js.get('backoffLimit', 4) > 3:
            raise ValueError('maintenance requires a finite total deadline and retry bound')
        if jp.get('restartPolicy') != 'Never' or jp.get('terminationGracePeriodSeconds') != 30:
            raise ValueError('unexpected maintenance restart/termination contract')
        if labels.get(COMPONENT) != 'maintenance' or any(matches(e, labels) for e in endpoints):
            raise ValueError('maintenance must not be an HTTP endpoint or PDB runtime')
        if len(jp['containers']) != 1:
            raise ValueError('bootstrap main containers must not run concurrently')
        for container in jp.get('initContainers', []) + jp['containers']:
            if container.get('restartPolicy'):
                raise ValueError('bootstrap init must complete, not run as a sidecar')
            if env_value(container, 'HEART_DB_MAX_CONNECTIONS') != 1:
                raise ValueError('each maintenance process requires pool cap one')
            if container['image'] != runtime[0]['image']:
                raise ValueError('maintenance must use the same reviewed Heart image')
            limits = container.get('resources', {}).get('limits', {})
            if limits.get('cpu') != runtime[0]['resources']['limits']['cpu'] or limits.get('memory') != runtime[0]['resources']['limits']['memory']:
                raise ValueError('maintenance must use the bounded runtime CPU/memory envelope')
            if not limits.get('cpu') or not limits.get('memory'):
                raise ValueError('maintenance requires CPU and memory limits')
            commands.append(container['args'])
    if sorted(weights) != [-4, -2]:
        raise ValueError('migration must precede bootstrap')
    migration = next(j for j in jobs if j['metadata']['annotations']['helm.sh/hook-weight'] == '-4')
    definition = next(j for j in jobs if j is not migration)
    mp, dp = migration['spec']['template']['spec'], definition['spec']['template']['spec']
    if mp.get('initContainers') or mp['containers'][0]['args'] != ['migrate']:
        raise ValueError('unexpected migration command')
    chain = [c['args'] for c in dp.get('initContainers', []) + dp['containers']]
    expected = [['bootstrap-definition', '/definitions/event-planning.yaml']]
    if dp.get('initContainers'):
        expected += [['bootstrap-definition', '/definitions/user-task-pilot.yaml']]
    if chain != expected:
        raise ValueError('definition bootstrap must finish event planning before optional pilot')
    configmaps = [d for d in heart if d['kind'] == 'ConfigMap']
    if len(configmaps) != 1 or configmaps[0]['metadata']['annotations'].get('helm.sh/hook-weight') != '-3':
        raise ValueError('definition source must remain between migration and bootstrap')
    return {'release': deployment['metadata']['labels']['app.kubernetes.io/instance'],
            'runtimeReplicas': replicas, 'runtimePoolPerReplica': pool,
            'runtimeConnections': replicas * pool, 'maintenanceConnections': 1,
            'bootstrapSteps': len(chain)}


def combined_budget(releases, limit=8):
    if len(releases) != 2 or len({r['release'] for r in releases}) != 2:
        raise ValueError('requires two different rendered releases')
    runtime = sum(r['runtimeConnections'] for r in releases)
    maintenance = sum(r['maintenanceConnections'] for r in releases)
    total = runtime + maintenance
    if total > limit:
        raise ValueError(f'Heart runtime {runtime} + maintenance {maintenance} = {total} exceeds role cap {limit}')
    return {'releases': releases, 'runtimeConnections': runtime,
            'maintenanceConnections': maintenance, 'plannedPeak': total, 'roleLimit': limit,
            'unbudgetedClientsAllowedAtPeak': limit-total,
            'assumptions': ['full Argo sync, one operation per release', 'no orphan/previous hook pods or manual duplicate runtime clients',
                            'controlled Deployment updates; manual deletion/force replacement is outside Recreate guarantee']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('production', type=Path); parser.add_argument('canary', type=Path)
    parser.add_argument('--role-limit', type=int, default=8)
    args = parser.parse_args()
    if not 1 <= args.role_limit <= 100:
        parser.error('role limit must be between 1 and 100')
    try:
        rows = [release_budget(list(yaml.safe_load_all(path.read_text(encoding='utf-8-sig')))) for path in (args.production,args.canary)]
        print(json.dumps(combined_budget(rows,args.role_limit),indent=2))
    except (ValueError,KeyError,TypeError) as error:
        parser.exit(1,f'Heart budget invalid: {error}\n')


if __name__ == '__main__':
    main()