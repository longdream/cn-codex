/**
 * Table commands.
 */

Protocol.register('table.insert', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var rows = params.rows || 2;
  var cols = params.cols || 2;

  var range;
  if (params.position !== undefined) {
    range = doc.Range(params.position, params.position);
  } else {
    range = wps.Application.Selection.Range;
  }

  var table = doc.Tables.Add(range, rows, cols);

  // Fill cells if data provided.
  if (params.data && Array.isArray(params.data)) {
    for (var r = 0; r < params.data.length && r < rows; r++) {
      var row = params.data[r];
      if (!Array.isArray(row)) continue;
      for (var c = 0; c < row.length && c < cols; c++) {
        table.Cell(r + 1, c + 1).Range.Text = String(row[c]);
      }
    }
  }

  return { success: true, rows: rows, cols: cols };
});

Protocol.register('table.setCell', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var tableIndex = params.tableIndex || 1;
  var row = params.row;
  var col = params.col;
  var text = params.text;

  if (row === undefined || col === undefined) {
    throw new Error('Missing required parameters: row, col');
  }
  if (text === undefined) throw new Error('Missing required parameter: text');

  if (tableIndex > doc.Tables.Count) {
    throw new Error('Table index out of range');
  }

  var table = doc.Tables.Item(tableIndex);
  table.Cell(row, col).Range.Text = String(text);
  return { success: true };
});

Protocol.register('table.getCell', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var tableIndex = params.tableIndex || 1;
  var row = params.row;
  var col = params.col;

  if (row === undefined || col === undefined) {
    throw new Error('Missing required parameters: row, col');
  }

  if (tableIndex > doc.Tables.Count) {
    throw new Error('Table index out of range');
  }

  var table = doc.Tables.Item(tableIndex);
  var cellText = table.Cell(row, col).Range.Text;
  // WPS cell text ends with \r\a, strip those control chars.
  cellText = cellText.replace(/[\r\n\x07]+$/, '');
  return { text: cellText, row: row, col: col };
});
