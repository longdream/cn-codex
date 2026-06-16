/**
 * Bookmark commands.
 */

Protocol.register('bookmark.add', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var name = params.name;
  if (!name) throw new Error('Missing required parameter: name');

  var range;
  if (params.start !== undefined && params.end !== undefined) {
    range = doc.Range(params.start, params.end);
  } else {
    range = wps.Application.Selection.Range;
  }

  doc.Bookmarks.Add(name, range);
  return { success: true, name: name };
});

Protocol.register('bookmark.goto', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var name = params.name;
  if (!name) throw new Error('Missing required parameter: name');

  if (!doc.Bookmarks.Exists(name)) {
    throw new Error('Bookmark not found: ' + name);
  }

  var bm = doc.Bookmarks.Item(name);
  bm.Range.Select();
  return { success: true };
});

Protocol.register('bookmark.list', function () {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');

  var bookmarks = [];
  var count = doc.Bookmarks.Count;
  for (var i = 1; i <= count; i++) {
    var bm = doc.Bookmarks.Item(i);
    bookmarks.push({
      name: bm.Name,
      start: bm.Range.Start,
      end: bm.Range.End,
      text: bm.Range.Text || '',
    });
  }
  return { bookmarks: bookmarks };
});

Protocol.register('bookmark.delete', function (params) {
  var doc = wps.Application.ActiveDocument;
  if (!doc) throw new Error('No active document');
  var name = params.name;
  if (!name) throw new Error('Missing required parameter: name');

  if (!doc.Bookmarks.Exists(name)) {
    throw new Error('Bookmark not found: ' + name);
  }

  doc.Bookmarks.Item(name).Delete();
  return { success: true };
});
