"""Contracts for the rendered shared Heart rollout budget, not application behavior."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import unittest
import yaml

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('heart_budget', ROOT/'tools/verify-heart-rollout-budget.py')
budget = importlib.util.module_from_spec(spec); spec.loader.exec_module(budget)


def render(name='sproyt', replicas=2, pilot=True, v2=False):
    settings = {'config.oidcIssuer':'https://sproyt-security.bjoroy.me/application/o/sproyt/',
      'config.oidcClientId':'ci-client', 'image.digest':'sha256:'+'a'*64,
      'heart.enabled':'true','heart.image.digest':'sha256:'+'b'*64,
      'heart.replicaCount':str(replicas),'config.processPilotEnabled':str(pilot).lower(),
      'config.processPilotAssigneeId':'00000000-0000-0000-0000-000000000001',
      'heart.runtimeV2.enabled':str(v2).lower(),'config.processPilotRuntime':'v2' if v2 else 'v1'}
    command = [os.environ.get('HELM','helm'),'template',name,str(ROOT/'helm/sproyt'),'--namespace',name]
    for key,value in settings.items(): command += ['--set',f'{key}={value}']
    return list(yaml.safe_load_all(subprocess.check_output(command,text=True,encoding='utf-8')))


class HeartRolloutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.production=render(); cls.canary=render('sproyt-canary',1); cls.without_pilot=render(pilot=False)

    def test_two_release_peak_includes_maintenance_and_has_no_runtime_surge(self):
        result=budget.combined_budget([budget.release_budget(self.production),budget.release_budget(self.canary)])
        self.assertEqual((result['runtimeConnections'],result['maintenanceConnections'],result['plannedPeak']),(6,2,8))
        self.assertEqual(result['unbudgetedClientsAllowedAtPeak'],0)

    def test_pilot_off_has_one_step_and_pilot_on_is_init_then_main(self):
        self.assertEqual(budget.release_budget(self.without_pilot)['bootstrapSteps'],1)
        self.assertEqual(budget.release_budget(self.production)['bootstrapSteps'],2)

    def test_current_v2_worker_uses_the_same_bounded_runtime_pool(self):
        result=budget.combined_budget([budget.release_budget(render(v2=True)),
                                      budget.release_budget(render('sproyt-canary',1,v2=True))])
        self.assertEqual(result['plannedPeak'],8)

    def test_unsafe_replica_override_is_rejected_instead_of_raising_role_limit(self):
        with self.assertRaisesRegex(ValueError,'exceeds role cap'):
            budget.combined_budget([budget.release_budget(self.production),budget.release_budget(render('sproyt-canary',2))])

    def test_rolling_update_cannot_claim_a_strict_connection_bound(self):
        docs=copy.deepcopy(self.production)
        next(d for d in docs if d['kind']=='Deployment' and d['metadata']['name'].endswith('-heart'))['spec']['strategy']={'type':'RollingUpdate','rollingUpdate':{'maxSurge':0,'maxUnavailable':1}}
        with self.assertRaisesRegex(ValueError,'Recreate'): budget.release_budget(docs)

    def test_retry_waits_for_termination_and_has_finite_total_deadline(self):
        for key,value in [('podReplacementPolicy','TerminatingOrFailed'),('parallelism',2),('activeDeadlineSeconds',None)]:
            docs=copy.deepcopy(self.production)
            job=next(d for d in docs if d['kind']=='Job' and '-heart-' in d['metadata']['name'])
            if value is None: del job['spec'][key]
            else: job['spec'][key]=value
            with self.subTest(key=key), self.assertRaises(ValueError):budget.release_budget(docs)

    def test_parallel_bootstrap_and_restartable_init_are_rejected(self):
        for sidecar in [False,True]:
            docs=copy.deepcopy(self.production)
            job=next(d for d in docs if d['kind']=='Job' and '-heart-definitions-' in d['metadata']['name'])
            pod=job['spec']['template']['spec']
            if sidecar:pod['initContainers'][0]['restartPolicy']='Always'
            else:pod['containers'].extend(pod.pop('initContainers'))
            with self.subTest(sidecar=sidecar), self.assertRaises(ValueError):budget.release_budget(docs)

    def test_services_and_pdbs_never_select_maintenance_pods(self):
        for kind in ['Service','PodDisruptionBudget']:
            docs=copy.deepcopy(self.production)
            obj=next(d for d in docs if d['kind']==kind and d['metadata']['name'].endswith('-heart'))
            selector=obj['spec']['selector'] if kind=='Service' else obj['spec']['selector']['matchLabels']
            del selector[budget.COMPONENT]
            with self.subTest(kind=kind), self.assertRaisesRegex(ValueError,'runtime component'):budget.release_budget(docs)

    def test_live_deployment_immutable_selector_is_not_changed(self):
        docs=copy.deepcopy(self.production)
        deployment=next(d for d in docs if d['kind']=='Deployment' and d['metadata']['name'].endswith('-heart'))
        deployment['spec']['selector']['matchLabels'][budget.COMPONENT]='runtime'
        with self.assertRaisesRegex(ValueError,'immutable'):budget.release_budget(docs)

    def test_hook_pool_and_resource_limits_are_explicit(self):
        for mutate in [lambda c:c['env'].append({'name':'HEART_DB_MAX_CONNECTIONS','value':'2'}),
                       lambda c:c.pop('resources')]:
            docs=copy.deepcopy(self.production)
            job=next(d for d in docs if d['kind']=='Job' and '-heart-migrate-' in d['metadata']['name'])
            mutate(job['spec']['template']['spec']['containers'][0])
            with self.assertRaises(ValueError):budget.release_budget(docs)


if __name__=='__main__':unittest.main()