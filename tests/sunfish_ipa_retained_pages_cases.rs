#[cfg(feature = "sunfish-smmu-install-probe")]
#[test]
fn retained_receive_reports_each_page_and_rejects_corruption_in_all_eight() {
    let mut f = Fixture::new();
    let mut shared = super::shared_fixture::Fixture::new();
    let data: [u64; 32] = core::array::from_fn(|n| 0xa3000000 + n as u64 * 4096);
    let mut ram = receive_ram();
    let mut extra: [Page; 4] = core::array::from_fn(|_| Page([0xa5; 512]));
    let mut n = attach_receive(&mut f, &mut shared, &data, &mut ram);
    let plan = stage_receive(&mut n, &mut extra);
    n.select(&plan).unwrap();
    n.install_experiment(&plan, 8).unwrap();

    let (extra, _) = n.root_extra.unwrap();
    let (receive, _) = n.receive_table.unwrap();
    let pointers = [n.table_pointers[0], n.table_pointers[1], extra[0],
        n.table_pointers[2], extra[1], receive, extra[2], n.table_pointers[3]];
    let mut pages = Vec::new();
    let mut setup = Vec::new();
    n.retained_receive_domain_with(|stage, index| {
        if matches!(stage, RetainedDomainStage::TableSetup) { setup.push(index); }
        if matches!(stage, RetainedDomainStage::TablePage) { pages.push(index); }
    }).unwrap();
    assert_eq!(setup, vec![0, 1, 7, 8, 9, 10, 12, 13, 14, 15, 16, 11, 2, 3, 4, 5, 6]);
    assert_eq!(pages, (0..8).collect::<Vec<_>>());

    for (page, pointer) in pointers.into_iter().enumerate() {
        // SAFETY: fixture owns all eight pages; restore each word after the check.
        let old = unsafe { pointer.add(511).read_volatile() };
        unsafe { pointer.add(511).write_volatile(old ^ 1); }
        let mut last_page = None;
        let result = n.retained_receive_domain_with(|stage, index| {
            if matches!(stage, RetainedDomainStage::TablePage) { last_page = Some(index); }
        });
        assert!(matches!(result, Err(Error::Tables)), "page {page}");
        assert_eq!(last_page, Some(page));
        unsafe { pointer.add(511).write_volatile(old); }
        n.retained_receive_domain().unwrap();
    }
}
