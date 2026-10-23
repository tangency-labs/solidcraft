# Quarter-section plane selection

Start Section Analysis and select its first plane from the Browser or a planar model face. Enable **Quarter section** to add the **Second plane** input. It uses the same selection chip, active-input indication, clear button and plane/planar-face picker as the first input. Click either chip to replace that plane; choosing the second does not reset the first.

Offsets and flips remain independent. The existing quarter-section command requires perpendicular planes. Curved faces and missing construction planes are refused for either input. Turning quarter mode off temporarily retains the second draft choice so it can be restored when quarter mode is enabled again.

A quarter section removes the positive side of either plane and retains the intersection of their kept half-spaces. Selecting two adjacent lateral faces and moving both planes inward therefore removes both external strips, rather than just their shared corner. Each Flip reverses only its plane's kept side; it does not move either authored offset. A single-plane section is unchanged.

The toolbar starts a new analysis, following the current application shell. Use **Edit Section Analysis** in the Browser to reopen both authored plane groups. Command-made quarter cuts reopen with their two effective planes as current-plane references. Cancel leaves the existing analysis intact; Remove Section clears both cuts. Sections affect the view and picking, not saved solid geometry or volume.
