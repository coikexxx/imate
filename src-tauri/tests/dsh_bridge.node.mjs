import test from 'node:test';
import assert from 'node:assert/strict';
import { prepareRuntime, buildCatalog, publicFailure, readSourceRows } from '../src/dsh_bridge.mjs';

test('来源 profile 仅通过只读接口组合，不触发原生 profile 迁移写入',()=>{
  const boot = {
    resolveProfileDir: () => '/source/web',
    readProfileManifest: (_bin, dir) => dir === '/source/web' ? { dsh:{profile:{bundles:['base']}} } : {dsh:{bundle:{patch:'base.yml'}}},
    readProfileVersionExemptions: () => ({}),
    resolveBundleDir: () => '/bundle',
    assertBundlePlatform: () => {}, evaluatePluginCompatibility: () => undefined,
    bundlePatchPaths: () => ['/bundle/base.yml'],
    loadOverlayPatches: () => [{id:'base'}],
    loadOptionalPatches: (_bin, path) => [{id:path.replaceAll('\\','/').endsWith('web/cordis.patch.yml')?'web':'home'}],
    composeEntries: layers => layers.flat(),
    loadProfileDirectory: () => {throw new Error('不应调用可能写入迁移的接口');},
  };
  assert.deepEqual(readSourceRows(boot,{home:'/source',installAnchor:'/install'},'web').map(row=>row.id),['base','web','home']);
});

function fixture() {
  const updates = [];
  const rows = [
    { id: 'llm-pi-ai', config: { providers: { gateway: { apiKeyEnv: 'PRIVATE_REF', baseURL: 'https://example.invalid', models: [{ id: 'm', name: '自定义模型' }] } } } },
    { id: 'deepseek-account', config: { platformOrigin: 'https://example.invalid' } },
    { id: 'credentials', config: { storeFile: 'source-credentials.yml' } },
    { id: 'authorization', config: { issuer: 'source' } },
  ];
  const entries = ['llm-pi-ai', 'deepseek-account', 'credentials', 'authorization', 'headless-runner'].map(id => ({options:{id}, update:async value=>{updates.push({id,...value});}}));
  const ctx={root:{loader:{await:async()=>{},entries:()=>entries}},llm:{listProviders:()=>[{id:'gateway',name:'网关'},{id:'broken',name:'失败供应商'}],listModels:async id=>{if(id==='broken')throw new Error('token=私密信息');return [{id:'m',name:'自定义模型'}];}}};
  return {ctx,rows,updates};
}

test('所选自定义模型沿用来源 profile 的配置，运行准备不启动生成', async()=>{
  const {ctx,rows,updates}=fixture();
  await prepareRuntime(ctx,{operation:'run',profile:'web',selection:{profile:'web',provider:'gateway',model:'m'},authentication:'headless'},rows);
  assert.deepEqual(updates.find(row=>row.id==='llm-pi-ai').config.providers.gateway,rows[0].config.providers.gateway);
  assert.equal(updates.some(row=>row.id==='headless-runner'),false);
  assert.equal(updates.some(row=>row.id==='deepseek-account'&&row.disabled===false),false);
});

test('账号服务使用来源配置启动，目录故障逐供应商隔离且不泄漏凭据',async()=>{
  const {ctx,rows,updates}=fixture();
  await prepareRuntime(ctx,{operation:'catalog',profile:'web',authentication:'account'},rows);
  const account=updates.find(row=>row.id==='deepseek-account');
  assert.equal(account.disabled,false);assert.equal(account.config.platformOrigin,'https://example.invalid');
  assert.deepEqual(updates.find(row=>row.id==='credentials'), {id:'credentials',config:{storeFile:'source-credentials.yml'},disabled:false});
  assert.deepEqual(updates.find(row=>row.id==='authorization'), {id:'authorization',config:{issuer:'source'},disabled:false});
  assert.ok(updates.findIndex(row=>row.id==='credentials') < updates.indexOf(account));
  assert.equal(updates.some(row=>row.id==='headless-runner'),false);
  const catalog=await buildCatalog(ctx,'web');
  assert.equal(catalog.groups[0].models[0].name,'自定义模型');
  assert.equal(catalog.failures[0].provider,'broken');
  assert.equal(JSON.stringify(catalog).includes('私密信息'),false);
  assert.equal(JSON.stringify(catalog).includes('PRIVATE_REF'),false);
  assert.equal(publicFailure(new Error('secret')).includes('secret'),false);
});

test('配置删除或模型不再可用时明确失败，不回退到其他模型',async()=>{
  const {ctx,rows}=fixture();
  await assert.rejects(()=>prepareRuntime(ctx,{operation:'run',profile:'web',authentication:'headless',selection:{provider:'missing',model:'m'}},rows));
});
