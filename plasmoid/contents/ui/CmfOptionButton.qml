import QtQuick
import org.kde.kirigami as Kirigami

Item {
    id: root

    property string text: ""
    property bool selected: false

    signal clicked()

    implicitHeight: Kirigami.Units.gridUnit * 1.65

    Rectangle {
        id: background
        anchors.fill: parent
        radius: Kirigami.Units.gridUnit * 0.25
        color: root.selected
            ? Kirigami.Theme.highlightColor
            : Kirigami.ColorUtils.adjustColor(Kirigami.Theme.textColor, {"alpha": -210})
        border.width: root.selected || mouseArea.containsMouse ? 1 : 0
        border.color: root.selected
            ? Kirigami.Theme.highlightColor
            : Kirigami.ColorUtils.adjustColor(Kirigami.Theme.textColor, {"alpha": -170})
        opacity: mouseArea.pressed ? 0.8 : 1

        Behavior on color {
            ColorAnimation { duration: 120 }
        }

        Behavior on opacity {
            NumberAnimation { duration: 80 }
        }
    }

    Text {
        anchors.centerIn: parent
        width: parent.width - Kirigami.Units.smallSpacing * 2
        text: root.text
        color: root.selected ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
        elide: Text.ElideRight
        font.pixelSize: Kirigami.Units.gridUnit * 0.68
        font.weight: root.selected ? Font.DemiBold : Font.Medium
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    MouseArea {
        id: mouseArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
