import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.components as PlasmaComponents3

Card {
    id: root

    property var device: null
    property int pendingAncLevel: -1
    property int pendingEqPreset: -1
    readonly property int currentAncLevel: device?.nothing?.anc_level ?? 0
    readonly property int currentEqPreset: device?.nothing?.eq_preset ?? -1
    readonly property int displayedAncLevel: pendingAncLevel >= 0 ? pendingAncLevel : currentAncLevel
    readonly property int displayedEqPreset: pendingEqPreset >= 0 ? pendingEqPreset : currentEqPreset

    signal ancLevelChanged(level: int)
    signal eqPresetChanged(preset: int)
    signal ringToggled(enabled: bool)

    title: i18n("CMF Headphone Pro")
    implicitHeight: Kirigami.Units.gridUnit * 23

    onCurrentAncLevelChanged: {
        if (pendingAncLevel === currentAncLevel) {
            pendingAncLevel = -1
        }
    }

    onCurrentEqPresetChanged: {
        if (pendingEqPreset === currentEqPreset) {
            pendingEqPreset = -1
        }
    }

    Timer {
        id: pendingAncTimer
        interval: 1400
        onTriggered: root.pendingAncLevel = -1
    }

    Timer {
        id: pendingEqTimer
        interval: 1400
        onTriggered: root.pendingEqPreset = -1
    }

    contentItem: Component {
        ColumnLayout {
            spacing: Kirigami.Units.smallSpacing

            RowLayout {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing

                Image {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 5.5
                    Layout.preferredHeight: Kirigami.Units.gridUnit * 5.5
                    fillMode: Image.PreserveAspectFit
                    source: "../images/cmf-headphone-pro-black.png"
                    asynchronous: true
                    mipmap: true
                    smooth: true
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing

                    PlasmaComponents3.Label {
                        Layout.fillWidth: true
                        text: device?.model ?? device?.name ?? ""
                        font.bold: true
                        elide: Text.ElideRight
                    }

                    PlasmaComponents3.Label {
                        Layout.fillWidth: true
                        text: device?.nothing?.firmware ? i18n("Firmware %1", device.nothing.firmware) : ""
                        opacity: 0.7
                        font.pixelSize: Kirigami.Units.gridUnit * 0.65
                        elide: Text.ElideRight
                    }

                    PlasmaComponents3.Button {
                        Layout.alignment: Qt.AlignLeft
                        text: device?.nothing?.ringing ? i18n("Stop Ring") : i18n("Ring")
                        icon.name: device?.nothing?.ringing ? "media-playback-stop" : "notifications"
                        onClicked: root.ringToggled(!(device?.nothing?.ringing ?? false))
                    }
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: i18n("Noise Control")
                opacity: 0.7
                font.pixelSize: Kirigami.Units.gridUnit * 0.65
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Kirigami.Units.smallSpacing
                rowSpacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: [
                        { "text": i18n("Off"), "level": 1 },
                        { "text": i18n("Transparency"), "level": 2 },
                        { "text": i18n("High ANC"), "level": 4 },
                        { "text": i18n("Mid"), "level": 5 },
                        { "text": i18n("Low"), "level": 3 },
                        { "text": i18n("Adaptive"), "level": 6 }
                    ]

                    delegate: CmfOptionButton {
                        Layout.fillWidth: true
                        Layout.preferredHeight: implicitHeight
                        text: modelData.text
                        selected: root.displayedAncLevel === modelData.level
                        onClicked: {
                            root.pendingAncLevel = modelData.level
                            pendingAncTimer.restart()
                            root.ancLevelChanged(modelData.level)
                        }
                    }
                }
            }

            PlasmaComponents3.Label {
                Layout.fillWidth: true
                text: i18n("Equalizer")
                opacity: 0.7
                font.pixelSize: Kirigami.Units.gridUnit * 0.65
            }

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: Kirigami.Units.smallSpacing
                rowSpacing: Kirigami.Units.smallSpacing

                Repeater {
                    model: [
                        { "text": i18n("Dirac"), "preset": 0 },
                        { "text": i18n("Pop"), "preset": 3 },
                        { "text": i18n("Rock"), "preset": 1 },
                        { "text": i18n("Classical"), "preset": 5 },
                        { "text": i18n("Electronic"), "preset": 2 },
                        { "text": i18n("Vocals"), "preset": 4 }
                    ]

                    delegate: CmfOptionButton {
                        Layout.fillWidth: true
                        Layout.preferredHeight: implicitHeight
                        text: modelData.text
                        selected: root.displayedEqPreset === modelData.preset
                        onClicked: {
                            root.pendingEqPreset = modelData.preset
                            pendingEqTimer.restart()
                            root.eqPresetChanged(modelData.preset)
                        }
                    }
                }
            }
        }
    }
}
