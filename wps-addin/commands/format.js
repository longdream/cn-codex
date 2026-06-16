/**
 * Text formatting commands.
 */

Protocol.register('format.setFont', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  var range;
  if (params.start !== undefined && params.end !== undefined) {
    range = doc.Range(params.start, params.end);
  } else {
    range = wps.Application.Selection.Range;
  }

  var font = range.Font;
  if (params.name) font.Name = params.name;
  if (params.size) font.Size = params.size;
  if (params.bold !== undefined) font.Bold = params.bold ? -1 : 0;
  if (params.italic !== undefined) font.Italic = params.italic ? -1 : 0;
  if (params.underline !== undefined) font.Underline = params.underline ? 1 : 0;
  if (params.color) font.Color = parseColor(params.color);
  if (params.strikethrough !== undefined) font.StrikeThrough = params.strikethrough ? -1 : 0;

  return { success: true };
});

Protocol.register('format.setParagraph', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  var range;
  if (params.start !== undefined && params.end !== undefined) {
    range = doc.Range(params.start, params.end);
  } else {
    range = wps.Application.Selection.Range;
  }

  var para = range.ParagraphFormat;

  // Alignment: left=0, center=1, right=2, justify=3
  if (params.alignment !== undefined) {
    var alignMap = { left: 0, center: 1, right: 2, justify: 3 };
    para.Alignment = alignMap[params.alignment] !== undefined
      ? alignMap[params.alignment]
      : params.alignment;
  }

  if (params.lineSpacing) para.LineSpacing = params.lineSpacing;
  if (params.spaceBefore !== undefined) para.SpaceBefore = params.spaceBefore;
  if (params.spaceAfter !== undefined) para.SpaceAfter = params.spaceAfter;
  if (params.firstLineIndent !== undefined) para.FirstLineIndent = params.firstLineIndent;
  if (params.leftIndent !== undefined) para.LeftIndent = params.leftIndent;
  if (params.rightIndent !== undefined) para.RightIndent = params.rightIndent;

  return { success: true };
});

Protocol.register('format.setStyle', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var styleName = params.style;
  if (!styleName) throw new Error('Missing required parameter: style');

  var range;
  if (params.start !== undefined && params.end !== undefined) {
    range = doc.Range(params.start, params.end);
  } else {
    range = wps.Application.Selection.Range;
  }

  range.Style = styleName;
  return { success: true };
});

/**
 * Parse a color value. Accepts:
 * - RGB integer (e.g. 255)
 * - Hex string (e.g. "#FF0000")
 */
function parseColor(value) {
  if (typeof value === 'number') return value;
  if (typeof value === 'string' && value.charAt(0) === '#') {
    var hex = value.substring(1);
    var r = parseInt(hex.substring(0, 2), 16);
    var g = parseInt(hex.substring(2, 4), 16);
    var b = parseInt(hex.substring(4, 6), 16);
    // WPS uses BGR format.
    return b * 65536 + g * 256 + r;
  }
  return 0;
}
