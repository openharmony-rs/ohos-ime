import { appTasks, OhosAppContext, OhosPluginId } from '@ohos/hvigor-ohos-plugin';
import { getNode } from '@ohos/hvigor'
import * as fs from 'fs';
import * as path from 'path';

const rootNode = getNode(__filename);
rootNode.afterNodeEvaluate(node => {
    const appContext = node.getContext(OhosPluginId.OHOS_APP_PLUGIN) as OhosAppContext;
    const buildProfileOpt = appContext.getBuildProfileOpt();
    const signingConfigsPath = process.env["TEST_IME_SIGNING_CONFIG"];
    if (signingConfigsPath) {
        if (!fs.existsSync(signingConfigsPath)) {
            console.error("File referenced by TEST_IME_SIGNING_CONFIG does not exist!");
            return;
        }
        const basePath = path.dirname(signingConfigsPath);
        const signingConfigs = JSON.parse(fs.readFileSync(signingConfigsPath));
        for (const config of signingConfigs) {
            config.material.certpath = path.resolve(basePath, config.material.certpath);
            config.material.profile = path.resolve(basePath, config.material.profile);
            config.material.storeFile = path.resolve(basePath, config.material.storeFile);
        }
        buildProfileOpt['app']['signingConfigs'] = signingConfigs;
    } else {
        console.log('TEST_IME_SIGNING_CONFIG is not set: the HAP stays unsigned.');
    }
    appContext.setBuildProfileOpt(buildProfileOpt);
    // A feature module must match the bundle name and version of the app it's installed into.
    const appJson = appContext.getAppJsonOpt();
    const bundleName = process.env["TEST_IME_BUNDLE_NAME"];
    if (bundleName) {
        appJson['app']['bundleName'] = bundleName;
    }
    const versionCode = process.env["TEST_IME_VERSION_CODE"];
    if (versionCode) {
        appJson['app']['versionCode'] = parseInt(versionCode);
        appJson['app']['versionName'] = process.env["TEST_IME_VERSION_NAME"] ?? versionCode;
    }
    appContext.setAppJsonOpt(appJson);
})

export default {
    system: appTasks,  /* Built-in plugin of Hvigor. It cannot be modified. */
    plugins:[]         /* Custom plugin to extend the functionality of Hvigor. */
}
