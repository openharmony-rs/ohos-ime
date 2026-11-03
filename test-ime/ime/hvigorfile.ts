import { hapTasks, OhosHapContext, OhosPluginId } from '@ohos/hvigor-ohos-plugin';
import { getNode } from '@ohos/hvigor';

// TEST_IME_MODULE_TYPE=feature builds the IME as a feature module, to install it into the bundle of the app under
// test and sign it with that app's profile.
const moduleNode = getNode(__filename);
moduleNode.afterNodeEvaluate(node => {
    const moduleType = process.env["TEST_IME_MODULE_TYPE"];
    if (moduleType) {
        const hapContext = node.getContext(OhosPluginId.OHOS_HAP_PLUGIN) as OhosHapContext;
        const moduleJson = hapContext.getModuleJsonOpt();
        moduleJson['module']['type'] = moduleType;
        hapContext.setModuleJsonOpt(moduleJson);
    }
});

export default {
    system: hapTasks,
    plugins: []
}
