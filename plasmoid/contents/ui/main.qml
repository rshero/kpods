import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents3
import org.kde.plasma.plasma5support as Plasma5Support
import org.kde.kirigami as Kirigami
import org.kde.plasma.workspace.dbus as DBus

PlasmoidItem {
    id: root

    Plasmoid.icon: "audio-headphones"
    Plasmoid.status: root.targetPlasmoidStatus

    preferredRepresentation: compactRepresentation
    switchWidth: Kirigami.Units.gridUnit * 12
    switchHeight: Kirigami.Units.gridUnit * 12

    // ------------------------------------------------------------------
    // Service availability watcher
    // ------------------------------------------------------------------
    DBus.DBusServiceWatcher {
        id: serviceWatcher
        busType: DBus.BusType.Session
        watchedService: "org.kairpods"

        onRegisteredChanged: {
            if (registered) {
                console.log("Kpods service is available")
                managerProps.updateAll()
                syncAutoPlayPause()
            } else {
                console.log("Kpods service is not available")
                devices = {}
                selectedDevice = ""
                connectedCount = 0
                applyPlasmoidStatus()
            }
        }
    }

    // ------------------------------------------------------------------
    // D-Bus properties bridge (org.kairpods.manager)
    // ------------------------------------------------------------------
    DBus.Properties {
        id: managerProps
        busType: DBus.BusType.Session
        service: "org.kairpods"
        path: "/org/kairpods/manager"
        iface: "org.kairpods.manager"

        onRefreshed: syncFromProperties()

        onPropertiesChanged: function (ifaceName, changed, invalidated) {
            if (ifaceName !== "org.kairpods.manager")
                return

            if ((changed.Devices != null) ||
                invalidated.indexOf("Devices") !== -1) {
                syncFromProperties()
            }


            const changedConnectedCount = unwrapDBusValue(changed.ConnectedCount, null)
            if (changedConnectedCount != null) {
                connectedCount = changedConnectedCount
                applyPlasmoidStatus()
            } else if (invalidated.indexOf("ConnectedCount") !== -1) {
                managerProps.updateAll()
            }
        }
    }

    Timer {
        id: stateRefreshTimer
        interval: 2000
        repeat: true
        running: serviceWatcher.registered
        onTriggered: managerProps.updateAll()
    }

    // ------------------------------------------------------------------
    // Reactive state exposed to the UI
    // ------------------------------------------------------------------
    property var devices: ({})
    property string selectedDevice: ""
    property var currentDevice: devices[selectedDevice] || null
    property int connectedCount: 0
    property bool hasConnectedDevice: {
        if (connectedCount > 0) return true

        for (var address in devices) {
            if (devices[address]?.connected) {
                return true
            }
        }
        return false
    }
    property int targetPlasmoidStatus: serviceWatcher.registered && root.hasConnectedDevice
        ? PlasmaCore.Types.ActiveStatus
        : PlasmaCore.Types.HiddenStatus

    onTargetPlasmoidStatusChanged: applyPlasmoidStatus()

    function applyPlasmoidStatus() {
        if (Plasmoid.status !== targetPlasmoidStatus)
            Plasmoid.status = targetPlasmoidStatus
    }

    function unwrapDBusValue(value, fallbackValue) {
        if (value == null)
            return fallbackValue
        if (value.value != null)
            return value.value
        return value
    }

    function syncFromProperties() {
        if (!managerProps.properties) {
            console.log("Properties not ready yet")
            return
        }

        const props = managerProps.properties
        let raw = unwrapDBusValue(props.Devices, "[]")
        let list = []
        try {
            list = JSON.parse(raw || "[]")
        } catch (e) {
            console.error("Failed to parse devices:", e)
            list = []
        }

        updateDevicesList(list)

        const propertyConnectedCount = unwrapDBusValue(props.ConnectedCount, null)
        if (propertyConnectedCount != null) {
            connectedCount = propertyConnectedCount
        }

        applyPlasmoidStatus()
    }

    // ------------------------------------------------------------------
    // D-Bus helpers (fire-and-forget – updates come via PropertiesChanged)
    // ------------------------------------------------------------------
    function syncAutoPlayPause() {
        if (!serviceWatcher.registered) return
        DBus.SessionBus.asyncCall({
            service: "org.kairpods",
            path: "/org/kairpods/manager",
            iface: "org.kairpods.manager",
            member: "SetAutoPlayPause",
            arguments: [Plasmoid.configuration.autoPlayPause]
        })
    }

    Connections {
        target: Plasmoid.configuration
        function onAutoPlayPauseChanged() {
            syncAutoPlayPause()
        }
    }

    function sendCommand(action, params) {
        if (!selectedDevice || !serviceWatcher.registered) return
        DBus.SessionBus.asyncCall({
            service: "org.kairpods",
            path: "/org/kairpods/manager",
            iface: "org.kairpods.manager",
            member: "SendCommand",
            arguments: [selectedDevice, action, params]
        })
    }

    function connectDevice(address) {
        if (!serviceWatcher.registered) return
        DBus.SessionBus.asyncCall({
            service: "org.kairpods",
            path: "/org/kairpods/manager",
            iface: "org.kairpods.manager",
            member: "ConnectDevice",
            arguments: [address]
        })
    }

    function disconnectDevice(address) {
        if (!serviceWatcher.registered) return
        DBus.SessionBus.asyncCall({
            service: "org.kairpods",
            path: "/org/kairpods/manager",
            iface: "org.kairpods.manager",
            member: "DisconnectDevice",
            arguments: [address]
        })
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------
    function updateDevicesList(deviceList) {
        var newDevices = {}
        for (var i = 0; i < deviceList.length; ++i) {
            var d = deviceList[i]
            if (d && d.address)
                newDevices[d.address] = d
        }
        devices = newDevices

        if (!selectedDevice && deviceList.length > 0)
            selectedDevice = deviceList[0].address

        if (selectedDevice && !devices.hasOwnProperty(selectedDevice))
            selectedDevice = ""
    }

    // ------------------------------------------------------------------
    // UI representations
    // ------------------------------------------------------------------
    compactRepresentation: CompactView {
        device: root.currentDevice
        showPulseAnimation: Plasmoid.configuration.showPulseAnimation
        onClicked: root.expanded = !root.expanded
    }

    fullRepresentation: Item {
        Layout.preferredWidth: Kirigami.Units.gridUnit * 18
        Layout.preferredHeight: Kirigami.Units.gridUnit * 28

        // Service unavailable view
        ColumnLayout {
            anchors.centerIn: parent
            visible: !serviceWatcher.registered
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "face-sad"
                Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                Layout.preferredHeight: Kirigami.Units.iconSizes.huge
                Layout.alignment: Qt.AlignHCenter
            }

            PlasmaComponents3.Label {
                text: i18n("Kpods Service Unavailable")
                font.bold: true
                Layout.alignment: Qt.AlignHCenter
            }

            PlasmaComponents3.Label {
                text: i18n("The Kpods service is not installed or not running")
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 15
            }

            PlasmaComponents3.Button {
                text: i18n("Install Kpods")
                icon.name: "download"
                Layout.alignment: Qt.AlignHCenter
                onClicked: Qt.openUrlExternally("https://github.com/rshero/kpods")
            }
        }

        // Normal device view
        FullView {
            anchors.fill: parent
            visible: serviceWatcher.registered

            devices: root.devices
            selectedDevice: root.selectedDevice
            currentDevice: root.currentDevice

            onDeviceSelected: function (address) {
                root.selectedDevice = address
            }

            onNoiseControlChanged: function (mode) {
                root.sendCommand("set_noise_mode", { value: mode })
            }

            onNothingAncLevelChanged: function (level) {
                root.sendCommand("set_nothing_anc_level", { level: level })
            }

            onNothingEqPresetChanged: function (preset) {
                root.sendCommand("set_nothing_eq_preset", { preset: preset })
            }

            onNothingRingToggled: function (enabled) {
                root.sendCommand("set_nothing_ring", { enabled: enabled })
            }

            onFeatureToggled: function (feature, enabled) {
                root.sendCommand("set_feature", { feature: feature, enabled: enabled })
            }

            onRefreshRequested: managerProps.updateAll()
        }
    }

    Component.onCompleted: {
        if (serviceWatcher.registered) {
            managerProps.updateAll()
            syncAutoPlayPause()
        }
    }
}
