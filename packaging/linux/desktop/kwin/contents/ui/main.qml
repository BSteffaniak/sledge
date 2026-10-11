import QtQuick
import org.kde.kwin 3.0

// KWin 6 declarative companion. Startup/runtime checks fail closed on other ABIs.
// Fixed callDBus endpoint only. This is a same-user focus hint, NOT lock authority.
Item {
    id: bridge
    property string epoch: Date.now().toString() + '-' + Math.random().toString(16)
    property int sequence: 0
    function publish() {
        const window = workspace.activeWindow;
        const identity = window ? window.resourceClass : '';
        const category = 'kwin_resource_class'; // Exact native KWin property; protocol is not exposed.
        const snapshot = {
            version: 1, epoch: epoch, sequence: ++sequence,
            focus: identity ? {category: category, value: identity} : null,
            lock_hint: 'unknown', generation: '', sources: [], current: null
        };
        publisher.arguments = [JSON.stringify(snapshot)];
        publisher.call();
    }
    DBusCall {
        id: publisher
        service: 'org.sledge.DesktopBridge'
        path: '/org/sledge/DesktopBridge'
        dbusInterface: 'org.sledge.DesktopBridge1'
        method: 'Publish'
    }
    Connections {
        target: workspace
        function onWindowActivated() { bridge.publish(); }
    }
    Connections {
        target: workspace.activeWindow
        ignoreUnknownSignals: true
        function onWindowClassChanged() { bridge.publish(); }
    }
    Timer { interval: 200; running: true; repeat: true; onTriggered: bridge.publish() }
    Component.onCompleted: {
        if (!('activeWindow' in workspace))
            throw new Error('Sledge requires KWin 6 activeWindow ABI');
        bridge.publish();
    }
}
