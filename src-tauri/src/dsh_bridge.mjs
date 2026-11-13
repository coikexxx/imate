import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

export const name = 'imates-dsh-bridge';
export const inject = ['llm', 'profileContext'];

// 不向宿主回传 SDK 原始错误，错误可能包含地址、请求头或配置值。
export function publicFailure() { return '无法读取该供应商模型，请在 Dsh 中检查配置、登录与网络后刷新'; }
const safeLabel = value => typeof value === 'string' && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
const accountNeeded = config => config.operation === 'catalog' || config.selection?.provider === 'deepseek-account'
  || (!config.selection && config.authentication === 'account');

export function readSourceRows(boot, context, profile) {
  const dir = boot.resolveProfileDir(profile, context.home);
  // loadProfileDirectory 会迁移废弃 bundle；这里仅使用原生只读接口，保持来源配置原样。
  const manifest = boot.readProfileManifest('dsh', dir);
  const exemptions = boot.readProfileVersionExemptions(dir);
  const layers = (manifest.dsh?.profile?.bundles ?? []).flatMap(packageName => {
    const packageDir = boot.resolveBundleDir('dsh', packageName, context.installAnchor, dir);
    const bundleManifest = boot.readProfileManifest('dsh', packageDir);
    const bundle = bundleManifest.dsh?.bundle;
    if (!bundle) throw new Error('Dsh 来源 bundle 配置不可用');
    boot.assertBundlePlatform(bundle.platforms, process.platform);
    const issue = boot.evaluatePluginCompatibility(bundleManifest, exemptions);
    if (issue && !issue.exempted) throw new Error('Dsh 来源 bundle 版本不兼容');
    return boot.bundlePatchPaths(packageDir, bundle).map(path => boot.loadOverlayPatches('dsh', path));
  });
  return boot.composeEntries([...layers, boot.loadOptionalPatches('dsh', join(dir, 'cordis.patch.yml')) ?? [],
    boot.loadOptionalPatches('dsh', join(context.home, 'cordis.patch.yml')) ?? []]);
}

export async function prepareRuntime(ctx, config, rows) {
  if (!['web', 'headless'].includes(config.profile) || config.selectionError) throw new Error('Dsh 模型选择无效，请重新选择');
  const entries = [...ctx.root.loader.entries()];
  const target = id => entries.find(entry => entry.options.id === id);
  for (const id of ['credentials', 'authorization']) {
    const entry = target(id);
    if (!entry) throw new Error('Dsh 凭据与授权服务不可用，请核对安装');
    const source = rows.find(row => row.id === id);
    await entry.update({ config: source?.config ?? {}, disabled: false }, false, true);
  }
  await ctx.root.loader.await();
  for (const id of ['llm-pi-ai', 'llm-deepseek', 'llm-deepseek-account']) {
    const source = rows.find(row => row.id === id);
    const entry = target(id);
    if (source && entry) await entry.update({ config: source.config ?? {}, disabled: source.disabled === true }, false, true);
  }
  if (accountNeeded(config)) {
    const source = rows.find(row => row.id === 'deepseek-account');
    if (!source || !target('deepseek-account')) throw new Error('Dsh 账号服务不可用，请核对安装');
    // 一次更新同时写配置和启用，避免以其他环境的 issuer 初始化账号存储。
    await target('deepseek-account').update({ config: source.config ?? {}, disabled: false }, false, true);
  }
  await ctx.root.loader.await();
  if (config.selection) {
    const { provider, model } = config.selection;
    if (!ctx.llm.listProviders().some(entry => entry.id === provider)) throw new Error('所选 Dsh 供应商已不存在，请刷新模型列表');
    const models = await ctx.llm.listModels(provider);
    if (!models.some(entry => entry.id === model)) throw new Error('所选 Dsh 模型暂不可用，请在 Dsh 中检查配置与凭据');
  }
}

const safeEffort = value => typeof value === 'string' && /^[A-Za-z0-9_-]{1,32}$/.test(value);

/** 读取单个模型声明的推理强度。读不到时返回空，不影响模型本身出现在列表里。 */
export async function readEfforts(ctx, provider, model) {
  try {
    const reasoning = (await ctx.llm.resolveModelInfo(provider, model)).reasoning;
    if (!reasoning) return {};
    const efforts = reasoning.efforts.filter(effort => safeEffort(effort.id) && safeLabel(effort.name)).map(effort => ({ id: effort.id, name: effort.name }));
    return { efforts, ...(safeEffort(reasoning.defaultEffort) ? { defaultEffort: reasoning.defaultEffort } : {}) };
  } catch {
    return {};
  }
}

export async function buildCatalog(ctx, profile) {
  const results = await Promise.all(ctx.llm.listProviders().map(async provider => {
    try {
      if (!safeLabel(provider.id) || !safeLabel(provider.name)) throw new Error('无效供应商');
      const models = await ctx.llm.listModels(provider.id);
      const usable = models.filter(model => safeLabel(model.id) && safeLabel(model.name));
      return { group: { id: provider.id, name: provider.name, models: await Promise.all(usable.map(async model => ({
        id: model.id, name: model.name, ...await readEfforts(ctx, provider.id, model.id) }))) } };
    } catch {
      return { failure: { provider: safeLabel(provider.id) ? provider.id : 'unknown', message: publicFailure() } };
    }
  }));
  return { type: 'imates_dsh_catalog', profile, groups: results.flatMap(result => result.group?.models.length ? [result.group] : []),
    failures: results.flatMap(result => result.failure ? [result.failure] : []) };
}

async function run(ctx, config) {
  try {
    await ctx.root.loader.await();
    const require = createRequire(ctx.profileContext.installAnchor);
    const boot = await import(pathToFileURL(require.resolve('@deepseek-ai/dsh-app-boot')).href);
    const rows = readSourceRows(boot, ctx.profileContext, config.profile);
    await prepareRuntime(ctx, config, rows);
    if (config.operation === 'catalog') {
      process.stdout.write(JSON.stringify(await buildCatalog(ctx, config.profile)) + '\n');
      ctx.get('appExit')(0);
    } else {
      const runner = [...ctx.root.loader.entries()].find(entry => entry.options.id === 'headless-runner');
      if (!runner) throw new Error('未安装 headless runner');
      await runner.update({ disabled: false }, false, true);
    }
  } catch {
    process.stdout.write(JSON.stringify({ type: 'error', message: 'Dsh 配置或模型准备失败，请刷新模型列表，并在 Dsh 中检查 profile、登录和凭据' }) + '\n');
    ctx.get('appExit')(1);
  }
}

// 先返回以让 Loader 完成初始化；异步工作等所有依赖就绪后再执行。
export function apply(ctx, config) { void run(ctx, config); }
